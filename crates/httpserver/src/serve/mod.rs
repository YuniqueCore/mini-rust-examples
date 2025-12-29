use std::path::{Path, PathBuf};

use anyhow::Result;

mod common;
mod request;
mod response;

pub use common::*;
pub use response::*;
use smol::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener as SmolTcpListener};

use crate::serve::request::Request;

#[derive(Debug)]
pub struct StaticServeService {
    /// Base directory to serve files from.
    serve_path: PathBuf,
}

impl StaticServeService {
    pub fn new(serve_path: &Path) -> Self {
        Self {
            serve_path: serve_path.to_path_buf(),
        }
    }

    pub async fn serve(&self, tcp_listener: SmolTcpListener) -> Result<()> {
        loop {
            let (stream, peer) = tcp_listener.accept().await?;
            log::debug!("Accepted connection from {peer}");

            let serve_path = self.serve_path.clone();
            smol::spawn(async move {
                if let Err(e) = handle_connection(stream, peer, serve_path).await {
                    log::debug!("Connection {peer} closed with error: {e:#}");
                }
            })
            .detach();
        }
    }
}

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

async fn handle_connection(
    mut stream: smol::net::TcpStream,
    peer: std::net::SocketAddr,
    serve_path: PathBuf,
) -> Result<()> {
    let request = match read_request(&mut stream, peer).await {
        Ok(req) => req,
        Err(e) => {
            let resp = Response::plain_text(400, "Bad Request", &format!("Bad Request: {e}\n"));
            let _ = stream.write_all(&resp.to_bytes()).await;
            let _ = stream.flush().await;
            return Ok(());
        }
    };

    let response = serve_static(&serve_path, &request).await;
    stream.write_all(&response.to_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_request(
    stream: &mut smol::net::TcpStream,
    peer: std::net::SocketAddr,
) -> Result<Request> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp).await?;
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
    let (req_no_body, content_length) = Request::parse_head(head, peer)?;

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
        return Ok(req_no_body);
    }

    let mut body: Vec<u8> = Vec::with_capacity(body_len);
    body.extend_from_slice(&buf[header_end..]);

    while body.len() < body_len {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow::anyhow!(
                "peer closed connection while reading body: {peer}"
            ));
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(body_len);

    Ok(req_no_body.with_body_bytes(body))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn serve_static(base: &Path, req: &Request) -> Response {
    match req.method {
        Method::GET | Method::HEAD => {}
        _ => {
            return Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD");
        }
    }

    let request_path = req.path.split('?').next().unwrap_or("/");
    let rel = request_path.trim_start_matches('/');

    let mut full = base.to_path_buf();
    for comp in std::path::Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(seg) => full.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::RootDir => {}
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Response::html(403, "Forbidden", "<h1>403 Forbidden</h1>");
            }
        }
    }

    let path = if full.is_dir() {
        let idx = full.join("index.html");
        if idx.is_file() {
            idx
        } else {
            return Response::html(404, "Not Found", "<h1>404 Not Found</h1>");
        }
    } else {
        full
    };

    let (bytes, content_type) = match std::fs::read(&path) {
        Ok(b) => (b, guess_content_type(&path)),
        Err(_) => return Response::html(404, "Not Found", "<h1>404 Not Found</h1>"),
    };

    let resp = Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", content_type);

    if matches!(req.method, Method::HEAD) {
        resp
    } else {
        resp.with_body_bytes(bytes)
    }
}

fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}
