use std::{
    io::SeekFrom,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use futures::io::AsyncSeekExt;
use http_range::HttpRange;
use smol::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::serve::{
    Method, Response, auth::BasicAuth, request::Request, shutdown::GracefulShutdown, static_files,
    types::TypeMappings,
};

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_CHUNK_LINE_BYTES: usize = 8 * 1024;
const MAX_TRAILER_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy)]
enum BodyKind {
    Empty,
    ContentLength(usize),
    Chunked,
}

struct ReadRequest {
    req: Request,
    body: BodyKind,
    pre_body: Vec<u8>,
}

struct BodyDecoder {
    kind: BodyKind,
    buf: Vec<u8>,
    remaining: usize,
    chunk_remaining: usize,
    done: bool,
    trailer_bytes: usize,
}

impl BodyDecoder {
    fn new(kind: BodyKind, pre_body: Vec<u8>) -> Self {
        let remaining = match kind {
            BodyKind::ContentLength(n) => n,
            _ => 0,
        };
        Self {
            kind,
            buf: pre_body,
            remaining,
            chunk_remaining: 0,
            done: matches!(kind, BodyKind::Empty),
            trailer_bytes: 0,
        }
    }

    async fn next_bytes(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<Option<Bytes>> {
        if self.done {
            return Ok(None);
        }
        match self.kind {
            BodyKind::Empty => {
                self.done = true;
                Ok(None)
            }
            BodyKind::ContentLength(_) => self.next_content_length(stream, shutdown).await,
            BodyKind::Chunked => self.next_chunked(stream, shutdown).await,
        }
    }

    async fn next_content_length(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<Option<Bytes>> {
        if self.remaining == 0 {
            self.done = true;
            return Ok(None);
        }

        if !self.buf.is_empty() {
            let n = self.buf.len().min(self.remaining);
            let out = Bytes::copy_from_slice(&self.buf[..n]);
            self.buf.drain(..n);
            self.remaining -= n;
            return Ok(Some(out));
        }

        let mut tmp = vec![0u8; 64 * 1024];
        let n = match read_or_shutdown(stream, &mut tmp, shutdown).await? {
            Some(n) => n,
            None => {
                self.done = true;
                return Ok(None);
            }
        };
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "peer closed connection while reading body",
            ));
        }
        let n = n.min(self.remaining);
        self.remaining -= n;
        Ok(Some(Bytes::copy_from_slice(&tmp[..n])))
    }

    async fn next_chunked(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<Option<Bytes>> {
        if self.chunk_remaining == 0 {
            let size = self.read_next_chunk_size(stream, shutdown).await?;
            if size == 0 {
                self.consume_trailers(stream, shutdown).await?;
                self.done = true;
                return Ok(None);
            }
            self.chunk_remaining = size;
        }

        self.ensure_buf_len(stream, shutdown, self.chunk_remaining + 2)
            .await?;

        let data: Vec<u8> = self.buf.drain(..self.chunk_remaining).collect();
        self.chunk_remaining = 0;

        if self.buf.first() != Some(&b'\r') || self.buf.get(1) != Some(&b'\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid chunked encoding: missing CRLF after chunk data",
            ));
        }
        self.buf.drain(..2);

        Ok(Some(Bytes::from(data)))
    }

    async fn read_next_chunk_size(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<usize> {
        loop {
            match httparse::parse_chunk_size(&self.buf) {
                Ok(httparse::Status::Complete((consumed, size))) => {
                    self.buf.drain(..consumed);
                    return usize::try_from(size).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "chunk size too large")
                    });
                }
                Ok(httparse::Status::Partial) => {
                    if self.buf.len() > MAX_CHUNK_LINE_BYTES {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "chunk size line too large",
                        ));
                    }
                    self.read_more(stream, shutdown).await?;
                }
                Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid chunk size",
                    ));
                }
            }
        }
    }

    async fn consume_trailers(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<()> {
        loop {
            // Empty trailer-part: last-chunk is followed by a single CRLF.
            if self.buf.first() == Some(&b'\r') && self.buf.get(1) == Some(&b'\n') {
                self.buf.drain(..2);
                return Ok(());
            }
            if let Some(pos) = find_subslice(&self.buf, b"\r\n\r\n") {
                self.buf.drain(..pos + 4);
                return Ok(());
            }
            if self.trailer_bytes > MAX_TRAILER_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "chunk trailers too large",
                ));
            }
            let before = self.buf.len();
            self.read_more(stream, shutdown).await?;
            self.trailer_bytes += self.buf.len().saturating_sub(before);
        }
    }

    async fn ensure_buf_len(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
        len: usize,
    ) -> std::io::Result<()> {
        while self.buf.len() < len {
            self.read_more(stream, shutdown).await?;
        }
        Ok(())
    }

    async fn read_more(
        &mut self,
        stream: &mut smol::net::TcpStream,
        shutdown: &GracefulShutdown,
    ) -> std::io::Result<()> {
        let mut tmp = [0u8; 64 * 1024];
        let n = match read_or_shutdown(stream, &mut tmp, shutdown).await? {
            Some(n) => n,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "shutdown while reading request body",
                ));
            }
        };
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "peer closed connection while reading request body",
            ));
        }
        self.buf.extend_from_slice(&tmp[..n]);
        Ok(())
    }
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
                        request.header_value("Range"),
                    )
                    .await?
                }
                static_files::RouteResult::Upload(_) => {
                    let resp =
                        Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                            .with_header("Allow", "GET, HEAD, PUT, POST");
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
                    .with_header("Allow", "GET, HEAD, PUT, POST");
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                resp.status
            }
            static_files::RouteResult::Upload(upload) => match upload.kind {
                static_files::UploadKind::PutFile => {
                    put_upload(
                        &mut stream,
                        &ctx.shutdown,
                        upload.path.as_path(),
                        read.body,
                        &read.pre_body,
                    )
                    .await?
                }
                static_files::UploadKind::MultipartDir => {
                    let resp =
                        Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                            .with_header("Allow", "GET, HEAD, PUT, POST");
                    stream.write_all(&resp.to_bytes()).await?;
                    stream.flush().await?;
                    resp.status
                }
            },
        },
        Method::POST => match static_files::route(&ctx.serve_path, &request, &ctx.types) {
            static_files::RouteResult::Response(resp) => {
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                resp.status
            }
            static_files::RouteResult::SendFile(_) => {
                let resp = Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                    .with_header("Allow", "GET, HEAD, PUT, POST");
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                resp.status
            }
            static_files::RouteResult::Upload(upload) => match upload.kind {
                static_files::UploadKind::MultipartDir => {
                    post_multipart_upload(
                        &mut stream,
                        &ctx.shutdown,
                        upload.path.as_path(),
                        &request,
                        read.body,
                        read.pre_body,
                    )
                    .await?
                }
                static_files::UploadKind::PutFile => {
                    let resp =
                        Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                            .with_header("Allow", "GET, HEAD, PUT, POST");
                    stream.write_all(&resp.to_bytes()).await?;
                    stream.flush().await?;
                    resp.status
                }
            },
        },
        _ => {
            let resp = Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD, PUT, POST");
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

    let is_chunked = req
        .header_value("Transfer-Encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let body = if is_chunked {
        BodyKind::Chunked
    } else if let Some(n) = content_length {
        BodyKind::ContentLength(n)
    } else {
        BodyKind::Empty
    };

    Ok(Some(ReadRequest {
        req,
        body,
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
    range_header: Option<&str>,
) -> Result<u16> {
    let (status, start, to_send, content_range) = match range_header {
        None => (200u16, 0u64, len, None),
        Some(h) => match HttpRange::parse(h, len) {
            Ok(ranges) if ranges.len() == 1 => {
                let r = &ranges[0];
                let start = r.start;
                let to_send = r.length;
                let end = start + to_send.saturating_sub(1);
                let content_range = format!("bytes {start}-{end}/{len}");
                (206u16, start, to_send, Some(content_range))
            }
            Ok(_) => {
                // Multiple ranges are not supported by this minimal server.
                let resp = Response::new()
                    .with_status(416, "Range Not Satisfiable")
                    .with_header("Content-Range", format!("bytes */{len}"))
                    .with_header("Content-Type", "text/plain; charset=utf-8")
                    .with_body_bytes(b"Range Not Satisfiable\n".to_vec());
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                return Ok(resp.status);
            }
            Err(_) => {
                let resp = Response::new()
                    .with_status(416, "Range Not Satisfiable")
                    .with_header("Content-Range", format!("bytes */{len}"))
                    .with_header("Content-Type", "text/plain; charset=utf-8")
                    .with_body_bytes(b"Range Not Satisfiable\n".to_vec());
                stream.write_all(&resp.to_bytes()).await?;
                stream.flush().await?;
                return Ok(resp.status);
            }
        },
    };

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

    let mut head = Response::new()
        .with_status(
            status,
            if status == 206 {
                "Partial Content"
            } else {
                "OK"
            },
        )
        .with_header("Accept-Ranges", "bytes")
        .with_header("Content-Type", content_type)
        .with_header("Content-Length", to_send.to_string());
    if let Some(cr) = content_range {
        head = head.with_header("Content-Range", cr);
    }
    stream.write_all(&head.to_bytes()).await?;

    if let Some(file) = file.as_mut() {
        let mut buf = vec![0u8; 64 * 1024];
        if start != 0 {
            file.seek(SeekFrom::Start(start)).await?;
        }
        let mut remaining = to_send;
        while remaining > 0 {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let n = n.min(remaining as usize);
            stream.write_all(&buf[..n]).await?;
            remaining -= n as u64;
        }
    }

    stream.flush().await?;
    Ok(status)
}

async fn put_upload(
    stream: &mut smol::net::TcpStream,
    shutdown: &GracefulShutdown,
    dest: &std::path::Path,
    body: BodyKind,
    pre_body: &[u8],
) -> Result<u16> {
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

    let bytes_written = match body {
        BodyKind::ContentLength(n) => write_body_to_file(
            &mut BodyDecoder::new(body, pre_body.to_vec()),
            stream,
            shutdown,
            &mut file,
        )
        .await?
        .min(n as u64),
        BodyKind::Chunked => {
            write_body_to_file(
                &mut BodyDecoder::new(body, pre_body.to_vec()),
                stream,
                shutdown,
                &mut file,
            )
            .await?
        }
        BodyKind::Empty => {
            let _ = smol::fs::remove_file(&tmp_path).await;
            let resp = Response::plain_text(411, "Length Required", "Length Required\n");
            stream.write_all(&resp.to_bytes()).await?;
            stream.flush().await?;
            return Ok(resp.status);
        }
    };

    file.flush().await?;

    if smol::fs::metadata(dest).await.is_ok() {
        let _ = smol::fs::remove_file(dest).await;
    }
    if let Err(e) = smol::fs::rename(&tmp_path, dest).await {
        let _ = smol::fs::remove_file(&tmp_path).await;
        return Err(anyhow::anyhow!("rename upload temp failed: {e}"));
    }

    let resp = if existed {
        Response::plain_text(200, "OK", &format!("OK ({bytes_written} bytes)\n"))
    } else {
        Response::plain_text(
            201,
            "Created",
            &format!("Created ({bytes_written} bytes)\n"),
        )
    };
    stream.write_all(&resp.to_bytes()).await?;
    stream.flush().await?;
    Ok(resp.status)
}

async fn write_body_to_file(
    decoder: &mut BodyDecoder,
    stream: &mut smol::net::TcpStream,
    shutdown: &GracefulShutdown,
    file: &mut smol::fs::File,
) -> Result<u64> {
    let mut written = 0u64;
    loop {
        match decoder.next_bytes(stream, shutdown).await {
            Ok(Some(bytes)) => {
                file.write_all(&bytes).await?;
                written += bytes.len() as u64;
            }
            Ok(None) => break,
            Err(e) => return Err(anyhow::anyhow!(e)),
        }
    }
    Ok(written)
}

async fn post_multipart_upload(
    stream: &mut smol::net::TcpStream,
    shutdown: &GracefulShutdown,
    dest_dir: &std::path::Path,
    req: &Request,
    body: BodyKind,
    pre_body: Vec<u8>,
) -> Result<u16> {
    let Some(content_type) = req.header_value("Content-Type") else {
        let resp = Response::plain_text(400, "Bad Request", "Missing Content-Type\n");
        stream.write_all(&resp.to_bytes()).await?;
        stream.flush().await?;
        return Ok(resp.status);
    };

    let boundary = match multer::parse_boundary(content_type) {
        Ok(b) => b,
        Err(_) => {
            let resp = Response::plain_text(400, "Bad Request", "Invalid multipart Content-Type\n");
            stream.write_all(&resp.to_bytes()).await?;
            stream.flush().await?;
            return Ok(resp.status);
        }
    };

    let shutdown = shutdown.clone();
    let init_state = (BodyDecoder::new(body, pre_body), stream.clone());
    let body_stream = futures::stream::unfold(init_state, move |(mut decoder, mut read_stream)| {
        let shutdown = shutdown.clone();
        async move {
            match decoder.next_bytes(&mut read_stream, &shutdown).await {
                Ok(Some(b)) => Some((Ok::<Bytes, std::io::Error>(b), (decoder, read_stream))),
                Ok(None) => None,
                Err(e) => Some((Err(e), (decoder, read_stream))),
            }
        }
    });

    let mut multipart = multer::Multipart::new(body_stream, boundary);
    let mut saved = Vec::new();
    while let Some(mut field) = multipart.next_field().await? {
        let Some(file_name) = field.file_name() else {
            continue;
        };
        let Some(safe_name) = sanitize_upload_file_name(file_name) else {
            continue;
        };
        let dest = dest_dir.join(&safe_name);
        let tmp_path = upload_tmp_path(&dest);
        let mut file = smol::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp_path)
            .await?;
        while let Some(chunk) = field.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;

        if smol::fs::metadata(&dest).await.is_ok() {
            let _ = smol::fs::remove_file(&dest).await;
        }
        smol::fs::rename(&tmp_path, &dest).await?;
        saved.push(safe_name);
    }

    drop(multipart);

    let resp = if saved.is_empty() {
        Response::plain_text(400, "Bad Request", "No files uploaded\n")
    } else {
        // 303 back to directory listing page.
        Response::new()
            .with_status(303, "See Other")
            .with_header("Location", req.path.clone())
            .with_header("Content-Type", "text/plain; charset=utf-8")
            .with_body_bytes(b"Uploaded\n".to_vec())
    };
    stream.write_all(&resp.to_bytes()).await?;
    stream.flush().await?;
    Ok(resp.status)
}

fn sanitize_upload_file_name(file_name: &str) -> Option<String> {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let base = std::path::Path::new(trimmed).file_name()?.to_string_lossy();
    if base == "." || base == ".." {
        return None;
    }
    if base.contains('/') || base.contains('\\') || base.contains('\0') {
        return None;
    }
    Some(base.to_string())
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
