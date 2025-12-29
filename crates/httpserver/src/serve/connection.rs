use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::Result;
use chrono::Utc;
use smol::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::serve::{
    Method, Response, auth::BasicAuth, request::Request, shutdown::GracefulShutdown, static_files,
    types::TypeMappings,
};

const MAX_HEADER_BYTES: usize = 32 * 1024;

struct ReadRequest {
    req: Request,
    content_length: Option<usize>,
    pre_body: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct ConnectionContext {
    pub serve_path: std::path::PathBuf,
    pub types: TypeMappings,
    pub auth: Option<BasicAuth>,
    pub started_at: Instant,
    pub request_count: Arc<AtomicU64>,
    pub shutdown: GracefulShutdown,
}

pub async fn handle(
    mut stream: smol::net::TcpStream,
    peer: std::net::SocketAddr,
    ctx: ConnectionContext,
) -> Result<()> {
    let read = match read_request(&mut stream, peer, &ctx.shutdown).await {
        Ok(Some(read)) => read,
        Ok(None) => return Ok(()),
        Err(e) => {
            let resp = Response::plain_text(400, "Bad Request", &format!("Bad Request: {e}\n"));
            let _ = stream.write_all(&resp.to_bytes()).await;
            let _ = stream.flush().await;
            return Ok(());
        }
    };

    let request = read.req;

    if let Some(auth) = &ctx.auth
        && !auth.is_authorized(&request)
    {
        let resp = auth.unauthorized_response();
        let _ = stream.write_all(&resp.to_bytes()).await;
        let _ = stream.flush().await;
        return Ok(());
    }

    let status = match request.method {
        Method::GET | Method::HEAD => {
            let is_head = matches!(request.method, Method::HEAD);
            match static_files::route(&ctx.serve_path, &request, &ctx.types) {
                static_files::RouteResult::Response(resp) => {
                    stream.write_all(&resp.to_bytes()).await?;
                    stream.flush().await?;
                    resp.status
                }
                static_files::RouteResult::SendFile(file) => {
                    send_file(
                        &mut stream,
                        file.path.as_path(),
                        &file.content_type,
                        file.len,
                        is_head,
                    )
                    .await?
                }
                static_files::RouteResult::Upload(_) => {
                    let resp =
                        Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                            .with_header("Allow", "GET, HEAD, PUT");
                    stream.write_all(&resp.to_bytes()).await?;
                    stream.flush().await?;
                    resp.status
                }
            }
        }
        Method::PUT => match static_files::route(&ctx.serve_path, &request, &ctx.types) {
            static_files::RouteResult::Response(resp) => {
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                resp.status
            }
            static_files::RouteResult::SendFile(_) => {
                let resp = Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                    .with_header("Allow", "GET, HEAD, PUT");
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                resp.status
            }
            static_files::RouteResult::Upload(upload) => {
                put_upload(
                    &mut stream,
                    &ctx.shutdown,
                    upload.path.as_path(),
                    read.content_length,
                    &read.pre_body,
                )
                .await?
            }
        },
        _ => {
            let resp = Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD, PUT");
            stream.write_all(&resp.to_bytes()).await?;
            stream.flush().await?;
            resp.status
        }
    };

    let count = ctx.request_count.fetch_add(1, Ordering::Relaxed) + 1;
    let elapsed = ctx.started_at.elapsed().as_secs_f64();
    let qps = if elapsed > 0.0 {
        (count as f64) / elapsed
    } else {
        0.0
    };
    log::info!(
        "{} peer={} method={} path={} status={} count={} qps={:.2}",
        Utc::now().to_rfc3339(),
        peer,
        request.method,
        request.path,
        status,
        count,
        qps
    );
    Ok(())
}

async fn read_request(
    stream: &mut smol::net::TcpStream,
    peer: std::net::SocketAddr,
    shutdown: &GracefulShutdown,
) -> Result<Option<ReadRequest>> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = match read_or_shutdown(stream, &mut tmp, shutdown).await? {
            Some(n) => n,
            None => return Ok(None),
        };
        if n == 0 {
            return Err(anyhow::anyhow!(
                "peer closed connection while reading request: {peer}"
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_HEADER_BYTES {
            return Err(anyhow::anyhow!("request headers too large"));
        }
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let pre_body = buf[header_end..].to_vec();
    let head = std::str::from_utf8(&buf[..header_end])?;
    let (req, content_length) = Request::parse_head(head)?;

    if req
        .header_value("Transfer-Encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        return Err(anyhow::anyhow!(
            "chunked transfer-encoding is not supported"
        ));
    }

    Ok(Some(ReadRequest {
        req,
        content_length,
        pre_body,
    }))
}

async fn read_or_shutdown(
    stream: &mut smol::net::TcpStream,
    buf: &mut [u8],
    shutdown: &GracefulShutdown,
) -> std::io::Result<Option<usize>> {
    let read_fut = async { stream.read(buf).await.map(Some) };
    let shutdown_fut = async {
        shutdown.wait_shutting_down().await;
        Ok(None)
    };
    future::or(read_fut, shutdown_fut).await
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn send_file(
    stream: &mut smol::net::TcpStream,
    path: &std::path::Path,
    content_type: &str,
    len: u64,
    is_head: bool,
) -> Result<u16> {
    let mut file = if is_head {
        None
    } else {
        match smol::fs::File::open(path).await {
            Ok(f) => Some(f),
            Err(_) => {
                let resp = Response::html(404, "Not Found", "<h1>404 Not Found</h1>");
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                return Ok(resp.status);
            }
        }
    };

    let head = Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", content_type)
        .with_header("Content-Length", len.to_string());
    stream.write_all(&head.to_bytes()).await?;

    if let Some(file) = file.as_mut() {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            stream.write_all(&buf[..n]).await?;
        }
    }

    stream.flush().await?;
    Ok(200)
}

async fn put_upload(
    stream: &mut smol::net::TcpStream,
    shutdown: &GracefulShutdown,
    dest: &std::path::Path,
    content_length: Option<usize>,
    pre_body: &[u8],
) -> Result<u16> {
    let Some(len) = content_length else {
        let resp = Response::plain_text(411, "Length Required", "Length Required\n");
        stream.write_all(&resp.to_bytes()).await?;
        stream.flush().await?;
        return Ok(resp.status);
    };

    let parent_ok = dest.parent().is_some_and(|p| p.is_dir());
    if !parent_ok {
        let resp = Response::plain_text(404, "Not Found", "Not Found\n");
        stream.write_all(&resp.to_bytes()).await?;
        stream.flush().await?;
        return Ok(resp.status);
    }

    let existed = dest.is_file();
    let tmp_path = upload_tmp_path(dest);
    let mut file = smol::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp_path)
        .await?;

    let mut remaining = len;
    if !pre_body.is_empty() {
        let n = pre_body.len().min(remaining);
        file.write_all(&pre_body[..n]).await?;
        remaining -= n;
    }

    let mut buf = [0u8; 64 * 1024];
    while remaining > 0 {
        let n = match read_or_shutdown(stream, &mut buf, shutdown).await? {
            Some(n) => n,
            None => {
                let _ = smol::fs::remove_file(&tmp_path).await;
                let resp =
                    Response::plain_text(503, "Service Unavailable", "Service Unavailable\n");
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                return Ok(resp.status);
            }
        };
        if n == 0 {
            let _ = smol::fs::remove_file(&tmp_path).await;
            return Err(anyhow::anyhow!(
                "peer closed connection while uploading to {}",
                dest.display()
            ));
        }
        let n = n.min(remaining);
        file.write_all(&buf[..n]).await?;
        remaining -= n;
    }

    file.flush().await?;

    if smol::fs::metadata(dest).await.is_ok() {
        let _ = smol::fs::remove_file(dest).await;
    }
    if let Err(e) = smol::fs::rename(&tmp_path, dest).await {
        let _ = smol::fs::remove_file(&tmp_path).await;
        return Err(anyhow::anyhow!("rename upload temp failed: {e}"));
    }

    let resp = if existed {
        Response::plain_text(200, "OK", "OK\n")
    } else {
        Response::plain_text(201, "Created", "Created\n")
    };
    stream.write_all(&resp.to_bytes()).await?;
    stream.flush().await?;
    Ok(resp.status)
}

fn upload_tmp_path(dest: &std::path::Path) -> std::path::PathBuf {
    let mut tmp = dest.to_path_buf();
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload".to_string());
    tmp.set_file_name(format!("{file_name}.uploading"));
    tmp
}
