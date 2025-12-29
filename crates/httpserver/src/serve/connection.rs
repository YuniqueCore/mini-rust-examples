use std::{
    path::Path,
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
    Response, request::Request, shutdown::GracefulShutdown, static_files, types::TypeMappings,
};

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub async fn handle(
    mut stream: smol::net::TcpStream,
    peer: std::net::SocketAddr,
    serve_path: &Path,
    types: &TypeMappings,
    started_at: Instant,
    request_count: Arc<AtomicU64>,
    shutdown: GracefulShutdown,
) -> Result<()> {
    let request = match read_request(&mut stream, peer, &shutdown).await {
        Ok(Some(req)) => req,
        Ok(None) => return Ok(()),
        Err(e) => {
            let resp = Response::plain_text(400, "Bad Request", &format!("Bad Request: {e}\n"));
            let _ = stream.write_all(&resp.to_bytes()).await;
            let _ = stream.flush().await;
            return Ok(());
        }
    };

    let response = static_files::serve_static(serve_path, &request, types);
    stream.write_all(&response.to_bytes()).await?;
    stream.flush().await?;

    let count = request_count.fetch_add(1, Ordering::Relaxed) + 1;
    let elapsed = started_at.elapsed().as_secs_f64();
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
        response.status,
        count,
        qps
    );
    Ok(())
}

async fn read_request(
    stream: &mut smol::net::TcpStream,
    peer: std::net::SocketAddr,
    shutdown: &GracefulShutdown,
) -> Result<Option<Request>> {
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

    let head = std::str::from_utf8(&buf[..header_end])?;
    let (req_no_body, content_length) = Request::parse_head(head)?;

    if req_no_body
        .header_value("Transfer-Encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        return Err(anyhow::anyhow!(
            "chunked transfer-encoding is not supported"
        ));
    }

    let body_len = content_length.unwrap_or(0);
    if body_len > MAX_BODY_BYTES {
        return Err(anyhow::anyhow!("request body too large"));
    }

    if body_len == 0 {
        return Ok(Some(req_no_body));
    }

    let mut body: Vec<u8> = Vec::with_capacity(body_len);
    body.extend_from_slice(&buf[header_end..]);

    while body.len() < body_len {
        let n = match read_or_shutdown(stream, &mut tmp, shutdown).await? {
            Some(n) => n,
            None => return Ok(None),
        };
        if n == 0 {
            return Err(anyhow::anyhow!(
                "peer closed connection while reading body: {peer}"
            ));
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(body_len);

    Ok(Some(req_no_body.with_body_bytes(body)))
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
