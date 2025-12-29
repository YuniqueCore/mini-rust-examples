use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::Result;
use chrono::Utc;
use mea::{condvar::Condvar, mutex::Mutex};

mod common;
mod request;
mod response;

pub use common::*;
pub use response::*;
use smol::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener as SmolTcpListener,
};

use crate::serve::request::Request;

#[derive(Debug)]
pub struct StaticServeService {
    /// Base directory; static files are served from `${serve_path}/`.
    serve_path: PathBuf,
    started_at: Instant,
    request_count: Arc<AtomicU64>,
}

impl StaticServeService {
    pub fn new(serve_path: &Path) -> Self {
        Self {
            serve_path: serve_path.to_path_buf(),
            started_at: Instant::now(),
            request_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn serve(
        &self,
        tcp_listener: SmolTcpListener,
        shutdown: GracefulShutdown,
    ) -> Result<()> {
        loop {
            let (stream, peer) = tcp_listener.accept().await?;
            log::debug!("Accepted connection from {peer}");

            if shutdown.is_shutting_down().await {
                drop(stream);
                break;
            }

            let serve_path = self.serve_path.clone();
            let started_at = self.started_at;
            let request_count = self.request_count.clone();
            let shutdown = shutdown.clone();
            smol::spawn(async move {
                shutdown.inflight_add(1).await;
                if let Err(e) = handle_connection(
                    stream,
                    peer,
                    serve_path,
                    started_at,
                    request_count,
                    shutdown.clone(),
                )
                .await
                {
                    log::debug!("Connection {peer} closed with error: {e:#}");
                }
                shutdown.inflight_sub(1).await;
            })
            .detach();
        }

        shutdown.wait_inflight_zero().await;
        Ok(())
    }
}

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
struct ShutdownState {
    shutting_down: bool,
    inflight: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct GracefulShutdown {
    inner: Arc<GracefulShutdownInner>,
}

#[derive(Debug)]
struct GracefulShutdownInner {
    state: Mutex<ShutdownState>,
    cv: Condvar,
}

impl GracefulShutdown {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GracefulShutdownInner {
                state: Mutex::new(ShutdownState::default()),
                cv: Condvar::new(),
            }),
        }
    }

    pub async fn initiate(&self) {
        let mut state = self.inner.state.lock().await;
        if state.shutting_down {
            return;
        }
        state.shutting_down = true;
        self.inner.cv.notify_all();
    }

    pub async fn is_shutting_down(&self) -> bool {
        self.inner.state.lock().await.shutting_down
    }

    pub async fn wait_shutting_down(&self) {
        let mut state = self.inner.state.lock().await;
        while !state.shutting_down {
            state = self.inner.cv.wait(state).await;
        }
    }

    pub async fn inflight_add(&self, n: u64) {
        let mut state = self.inner.state.lock().await;
        state.inflight = state.inflight.saturating_add(n);
        self.inner.cv.notify_all();
    }

    pub async fn inflight_sub(&self, n: u64) {
        let mut state = self.inner.state.lock().await;
        state.inflight = state.inflight.saturating_sub(n);
        self.inner.cv.notify_all();
    }

    pub async fn wait_inflight_zero(&self) {
        let mut state = self.inner.state.lock().await;
        while state.inflight != 0 {
            state = self.inner.cv.wait(state).await;
        }
    }
}

async fn handle_connection(
    mut stream: smol::net::TcpStream,
    peer: std::net::SocketAddr,
    serve_path: PathBuf,
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

    let response = serve_static(&serve_path, &request).await;
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

async fn serve_static(base: &Path, req: &Request) -> Response {
    match req.method {
        Method::GET | Method::HEAD => {}
        _ => {
            return Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD");
        }
    }

    let is_head = matches!(req.method, Method::HEAD);
    let raw_path = req.path.split('?').next().unwrap_or("/");
    let decoded_path = match percent_decode_path(raw_path) {
        Ok(p) => p,
        Err(_) => {
            return Response::html(400, "Bad Request", "<h1>400 Bad Request</h1>")
                .without_body_if(is_head);
        }
    };

    let request_path = decoded_path.as_str();
    let rel = request_path.trim_start_matches('/');

    let mut full = base.to_path_buf();
    for comp in std::path::Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(seg) => full.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::RootDir => {}
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Response::html(403, "Forbidden", "<h1>403 Forbidden</h1>")
                    .without_body_if(is_head);
            }
        }
    }

    if full.is_dir() {
        // Redirect to trailing slash (like python http.server) so relative links work.
        if request_path != "/" && !request_path.ends_with('/') {
            let location = format!("{}/", raw_path);
            return Response::redirect_301(&location).without_body_if(is_head);
        }

        // For "/" we always show a directory listing (more convenient for learning and exploration).
        if request_path != "/" {
            let idx = full.join("index.html");
            if idx.is_file() {
                return serve_file(&idx, req);
            }
        }

        return directory_listing_response(&full, raw_path, request_path, is_head);
    }

    serve_file(&full, req)
}

fn serve_file(path: &Path, req: &Request) -> Response {
    let is_head = matches!(req.method, Method::HEAD);
    let (bytes, content_type) = match std::fs::read(path) {
        Ok(b) => (b, guess_content_type(path)),
        Err(_) => {
            return Response::html(404, "Not Found", "<h1>404 Not Found</h1>")
                .without_body_if(is_head);
        }
    };

    let resp = Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", content_type);

    if is_head {
        resp
    } else {
        resp.with_body_bytes(bytes)
    }
}

fn directory_listing_response(
    dir: &Path,
    raw_url_path: &str,
    decoded_url_path: &str,
    is_head: bool,
) -> Response {
    let mut entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(_) => {
            return Response::html(404, "Not Found", "<h1>404 Not Found</h1>")
                .without_body_if(is_head);
        }
    };

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .file_name()
                .to_string_lossy()
                .cmp(&b.file_name().to_string_lossy()),
        }
    });

    let title = format!("Directory listing for {}", html_escape(decoded_url_path));
    let mut body = String::new();
    body.push_str("<style>body{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif} table{border-collapse:collapse} td{padding:4px 10px} a{text-decoration:none} a:hover{text-decoration:underline} .muted{color:#666}</style>");
    body.push_str(&format!("<h1>{}</h1>", title));
    body.push_str("<table>");
    body.push_str("<tr><td class=\"muted\">Name</td><td class=\"muted\">Size</td></tr>");

    if decoded_url_path != "/" {
        body.push_str("<tr><td><a href=\"../\">../</a></td><td class=\"muted\">-</td></tr>");
    }

    let href_prefix = ensure_trailing_slash_owned(raw_url_path);
    for ent in entries {
        let name_os = ent.file_name();
        let name = name_os.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }

        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let display_name = if is_dir {
            format!("{}/", html_escape(&name))
        } else {
            html_escape(&name)
        };

        let href = if is_dir {
            format!("{}{}/", href_prefix, url_escape_path_component(&name))
        } else {
            format!("{}{}", href_prefix, url_escape_path_component(&name))
        };

        let size = if is_dir {
            "-".to_string()
        } else {
            ent.metadata()
                .map(|m| m.len().to_string())
                .unwrap_or_else(|_| "-".to_string())
        };

        body.push_str(&format!(
            "<tr><td><a href=\"{href}\">{display}</a></td><td class=\"muted\">{size}</td></tr>",
            href = href,
            display = display_name,
            size = size
        ));
    }
    body.push_str("</table>");

    Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", "text/html; charset=utf-8")
        .with_body_bytes(format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body>{}</body></html>", title, body).into_bytes())
        .without_body_if(is_head)
}

fn ensure_trailing_slash_owned(url_path: &str) -> String {
    if url_path.ends_with('/') {
        url_path.to_string()
    } else {
        format!("{}/", url_path)
    }
}

fn percent_decode_path(path: &str) -> std::result::Result<String, ()> {
    // Path is expected to be ASCII-ish, but may contain UTF-8 percent-encoded bytes.
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(());
                }
                let hi = from_hex(bytes[i + 1]).ok_or(())?;
                let lo = from_hex(bytes[i + 2]).ok_or(())?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    std::str::from_utf8(&out)
        .map(|s| s.to_string())
        .map_err(|_| ())
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn url_escape_path_component(s: &str) -> String {
    // Minimal percent-encoding for path components.
    // Keep unreserved per RFC 3986: ALPHA / DIGIT / "-" / "." / "_" / "~"
    let mut out = String::new();
    for &b in s.as_bytes() {
        let is_unreserved =
            matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
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
        "rs" | "py" | "go" | "log" | "md" | "toml"  => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

trait ResponseHeadExt {
    fn without_body_if(self, is_head: bool) -> Self;
}

impl ResponseHeadExt for Response {
    fn without_body_if(self, is_head: bool) -> Self {
        if is_head { self.without_body() } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_decode_path() {
        assert_eq!(percent_decode_path("/a%20b").unwrap(), "/a b");
        assert_eq!(percent_decode_path("/%7Euser").unwrap(), "/~user");
        assert!(percent_decode_path("/%2").is_err());
        assert!(percent_decode_path("/%ZZ").is_err());
    }

    #[test]
    fn test_url_escape_path_component() {
        assert_eq!(url_escape_path_component("a b"), "a%20b");
        assert_eq!(url_escape_path_component("a/b"), "a%2Fb");
        assert_eq!(url_escape_path_component("~_-.a"), "~_-.a");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<a&\"'>"), "&lt;a&amp;&quot;&#39;&gt;");
    }
}
