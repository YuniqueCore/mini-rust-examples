use anyhow::Result;
use httparse::Header;
use smol::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use std::net::SocketAddr;

use crate::init::shutdown::GracefulShutdown;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug)]
struct ClientRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct UpstreamResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub async fn handle_local_target(bind_addr: SocketAddr, shutdown: &GracefulShutdown) -> Result<()> {
    let tcp_listener = TcpListener::bind(bind_addr).await?;
    log::info!("httproxy listening on {bind_addr}");

    loop {
        let Some((stream, peer)) = accept_or_shutdown(&tcp_listener, shutdown).await? else {
            break;
        };

        let shutdown = shutdown.clone();
        smol::spawn(async move {
            let _guard = shutdown.inflight_guard();
            if let Err(err) = handle_client(stream, peer).await {
                log::warn!("peer={peer} error: {err}");
            }
        })
        .detach();
    }

    shutdown.wait_inflight_zero().await;
    Ok(())
}

async fn accept_or_shutdown(
    listener: &TcpListener,
    shutdown: &GracefulShutdown,
) -> std::io::Result<Option<(TcpStream, SocketAddr)>> {
    let accept_fut = async { listener.accept().await.map(Some) };
    let shutdown_fut = async {
        shutdown.wait_shutting_down().await;
        Ok(None)
    };
    future::or(accept_fut, shutdown_fut).await
}

async fn handle_client(mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let req = match read_client_request(&mut stream, peer).await {
        Ok(req) => req,
        Err(err) => {
            write_plain_error(
                &mut stream,
                400,
                "Bad Request",
                format!("Bad Request: {err}\n"),
            )
            .await?;
            return Ok(());
        }
    };

    if req.method.eq_ignore_ascii_case("CONNECT") {
        let mut authority = req.path;
        if !authority.contains(':') {
            authority.push_str(":443");
        }

        let mut remote = match TcpStream::connect(authority.as_str()).await {
            Ok(s) => s,
            Err(err) => {
                write_plain_error(
                    &mut stream,
                    502,
                    "Bad Gateway",
                    format!("CONNECT failed: {err}\n"),
                )
                .await?;
                return Ok(());
            }
        };

        stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        stream.flush().await?;

        if !req.body.is_empty() {
            remote.write_all(&req.body).await?;
            remote.flush().await?;
        }

        log::info!("peer={peer} CONNECT {authority}");
        return tunnel(stream, remote).await;
    }

    log::info!("peer={peer} {} {}", req.method, req.path);

    match forward_via_ureq(req).await {
        Ok(resp) => write_response(&mut stream, &resp).await?,
        Err(err) => {
            log::debug!("peer={peer} upstream error: {err}");
            write_plain_error(
                &mut stream,
                502,
                "Bad Gateway",
                format!("Bad Gateway: {err}\n"),
            )
            .await?;
        }
    }

    Ok(())
}

async fn read_client_request(stream: &mut TcpStream, peer: SocketAddr) -> Result<ClientRequest> {
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];

    let header_end = loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("peer closed connection: {peer}"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_HEADER_BYTES {
            return Err(anyhow::anyhow!("request headers too large"));
        }
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let head = &buf[..header_end];

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(head)? {
        httparse::Status::Complete(_) => {}
        httparse::Status::Partial => return Err(anyhow::anyhow!("incomplete request headers")),
    }

    let method = req
        .method
        .ok_or_else(|| anyhow::anyhow!("missing method"))?;
    let path = req.path.ok_or_else(|| anyhow::anyhow!("missing path"))?;
    let _version = req
        .version
        .ok_or_else(|| anyhow::anyhow!("missing version"))?;

    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).to_string(),
            )
        })
        .collect();

    let pre_body = buf[header_end..].to_vec();
    if method.eq_ignore_ascii_case("CONNECT") {
        return Ok(ClientRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body: pre_body,
        });
    }

    if header_has_value(req.headers, "transfer-encoding", "chunked") {
        return Err(anyhow::anyhow!("chunked request body not supported"));
    }

    let content_length = parse_content_length(req.headers)?;
    let body = if let Some(len) = content_length {
        if len > MAX_BODY_BYTES {
            return Err(anyhow::anyhow!("request body too large: {len} bytes"));
        }

        let mut body = pre_body;
        while body.len() < len {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(anyhow::anyhow!("peer closed connection while reading body"));
            }
            body.extend_from_slice(&tmp[..n]);
            if body.len() > len {
                body.truncate(len);
                break;
            }
        }
        body.truncate(len);
        body
    } else {
        if !pre_body.is_empty() {
            log::debug!("peer={peer} extra bytes after headers are ignored (no Content-Length)");
        }
        Vec::new()
    };

    Ok(ClientRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    })
}

fn parse_content_length(headers: &[Header<'_>]) -> Result<Option<usize>> {
    let Some(h) = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
    else {
        return Ok(None);
    };
    let s = std::str::from_utf8(h.value)?.trim();
    if s.is_empty() {
        return Ok(None);
    }
    Ok(Some(s.parse()?))
}

fn header_has_value(headers: &[Header<'_>], name: &str, expected: &str) -> bool {
    headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case(name)
            && std::str::from_utf8(h.value)
                .ok()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case(expected))
    })
}

async fn forward_via_ureq(req: ClientRequest) -> Result<UpstreamResponse> {
    let url = build_target_url(&req.path, &req.headers)?;

    smol::unblock(move || {
        let mut builder = ureq::http::Request::builder()
            .method(req.method.as_str())
            .uri(url.as_str());

        for (name, value) in req.headers {
            if should_skip_request_header(&name) {
                continue;
            }
            builder = builder.header(name.as_str(), value.as_str());
        }

        builder = builder.header("accept-encoding", "identity");
        builder = builder.header("connection", "close");

        builder = builder.header("content-length", req.body.len().to_string());

        let request = builder.body(req.body)?;
        let resp = ureq::run(request)?;

        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect();

        let mut body = resp.into_body();
        let body = body.read_to_vec()?;

        Ok::<_, anyhow::Error>(UpstreamResponse {
            status,
            headers,
            body,
        })
    })
    .await
}

fn build_target_url(path: &str, headers: &[(String, String)]) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }

    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing Host header"))?;

    let path = if path.starts_with('/') || path == "*" {
        path.to_string()
    } else {
        format!("/{path}")
    };

    Ok(format!("http://{host}{path}"))
}

fn should_skip_request_header(name: &str) -> bool {
    is_hop_by_hop_header(name)
        || name.eq_ignore_ascii_case("accept-encoding")
        || name.eq_ignore_ascii_case("content-length")
}

fn should_skip_response_header(name: &str) -> bool {
    is_hop_by_hop_header(name)
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("content-encoding")
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
    )
}

async fn write_response(stream: &mut TcpStream, resp: &UpstreamResponse) -> Result<()> {
    let bytes = build_response_bytes(resp);
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

fn build_response_bytes(resp: &UpstreamResponse) -> Vec<u8> {
    let status = ureq::http::StatusCode::from_u16(resp.status)
        .unwrap_or(ureq::http::StatusCode::INTERNAL_SERVER_ERROR);
    let reason = status.canonical_reason().unwrap_or("");

    let mut out: Vec<u8> = Vec::with_capacity(1024 + resp.body.len());
    out.extend_from_slice(format!("HTTP/1.1 {} {reason}\r\n", status.as_u16()).as_bytes());

    for (name, value) in &resp.headers {
        if should_skip_response_header(name) {
            continue;
        }
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }

    out.extend_from_slice(format!("Content-Length: {}\r\n", resp.body.len()).as_bytes());
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(&resp.body);
    out
}

async fn write_plain_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: String,
) -> Result<()> {
    let bytes = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(bytes.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn tunnel(client: TcpStream, remote: TcpStream) -> Result<()> {
    let mut client_read = client.clone();
    let mut client_write = client;
    let mut remote_read = remote.clone();
    let mut remote_write = remote;

    let c2r = smol::spawn(async move { smol::io::copy(&mut client_read, &mut remote_write).await });
    let r2c = smol::spawn(async move { smol::io::copy(&mut remote_read, &mut client_write).await });

    let _ = c2r.await?;
    let _ = r2c.await?;
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_target_url_absolute() -> Result<()> {
        let headers = vec![("Host".to_string(), "example.com".to_string())];
        let url = build_target_url("http://example.com/a", &headers)?;
        assert_eq!(url, "http://example.com/a");
        Ok(())
    }

    #[test]
    fn test_build_target_url_origin_form() -> Result<()> {
        let headers = vec![("Host".to_string(), "example.com:8080".to_string())];
        let url = build_target_url("/hello", &headers)?;
        assert_eq!(url, "http://example.com:8080/hello");
        Ok(())
    }

    #[test]
    fn test_hop_by_hop_headers() {
        assert!(is_hop_by_hop_header("Connection"));
        assert!(is_hop_by_hop_header("proxy-connection"));
        assert!(is_hop_by_hop_header("TRANSFER-ENCODING"));
        assert!(!is_hop_by_hop_header("content-type"));
    }
}
