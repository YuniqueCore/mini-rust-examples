use std::path::{Path, PathBuf};

use crate::serve::{
    Method, Response, render,
    request::Request,
    types::{TypeAction, TypeMappings},
    url,
};

// Treat files >= 1024MiB as "large": avoid loading into memory for HTML rendering.
const LARGE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct FileToSend {
    pub path: PathBuf,
    pub content_type: String,
    pub len: u64,
}

#[derive(Debug)]
pub(crate) struct UploadTarget {
    pub path: PathBuf,
    pub kind: UploadKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UploadKind {
    PutFile,
    MultipartDir,
}

#[derive(Debug)]
pub(crate) enum RouteResult {
    Response(Response),
    SendFile(FileToSend),
    Upload(UploadTarget),
}

pub(crate) fn route(base: &Path, req: &Request, types: &TypeMappings) -> RouteResult {
    let is_head = matches!(req.method, Method::HEAD);
    let raw_path = req.path.split('?').next().unwrap_or("/");
    let decoded_path = match url::percent_decode_path(raw_path) {
        Ok(p) => p,
        Err(_) => {
            return RouteResult::Response(
                Response::html(400, "Bad Request", "<h1>400 Bad Request</h1>")
                    .without_body_if(is_head),
            );
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
                return RouteResult::Response(
                    Response::html(403, "Forbidden", "<h1>403 Forbidden</h1>")
                        .without_body_if(is_head),
                );
            }
        }
    }

    match req.method {
        Method::GET | Method::HEAD => {
            if full.is_dir() {
                if request_path != "/" && !request_path.ends_with('/') {
                    let location = format!("{}/", raw_path);
                    return RouteResult::Response(
                        Response::redirect_301(&location).without_body_if(is_head),
                    );
                }

                // Learning-friendly: always render a directory listing for directories.
                return RouteResult::Response(
                    directory_listing_response(&full, raw_path, request_path)
                        .without_body_if(is_head),
                );
            }

            serve_file(&full, req, types)
        }
        Method::PUT => {
            if request_path.ends_with('/') {
                return RouteResult::Response(
                    Response::plain_text(400, "Bad Request", "Bad Request\n")
                        .without_body_if(is_head),
                );
            }
            if full.is_dir() {
                return RouteResult::Response(
                    Response::plain_text(400, "Bad Request", "Bad Request\n")
                        .without_body_if(is_head),
                );
            }
            if !full.parent().is_some_and(|p| p.is_dir()) {
                return RouteResult::Response(
                    Response::plain_text(404, "Not Found", "Not Found\n").without_body_if(is_head),
                );
            }
            RouteResult::Upload(UploadTarget {
                path: full,
                kind: UploadKind::PutFile,
            })
        }
        Method::POST => {
            if !request_path.ends_with('/') || !full.is_dir() {
                return RouteResult::Response(
                    Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                        .with_header("Allow", "GET, HEAD, PUT, POST")
                        .without_body_if(is_head),
                );
            }
            RouteResult::Upload(UploadTarget {
                path: full,
                kind: UploadKind::MultipartDir,
            })
        }
        _ => RouteResult::Response(
            Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD, PUT, POST")
                .without_body_if(is_head),
        ),
    }
}

fn serve_file(path: &Path, req: &Request, types: &TypeMappings) -> RouteResult {
    let is_head = matches!(req.method, Method::HEAD);

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return RouteResult::Response(
                Response::html(404, "Not Found", "<h1>404 Not Found</h1>").without_body_if(is_head),
            );
        }
    };
    if !meta.is_file() {
        return RouteResult::Response(
            Response::html(404, "Not Found", "<h1>404 Not Found</h1>").without_body_if(is_head),
        );
    }

    let action = types.action_for_path(path);
    if meta.len() >= LARGE_FILE_BYTES {
        let content_type = match action {
            Some(TypeAction::Raw { ref content_type }) => content_type.clone(),
            _ => guess_content_type(path),
        };
        return RouteResult::SendFile(FileToSend {
            path: path.to_path_buf(),
            content_type,
            len: meta.len(),
        });
    }
    match action {
        Some(TypeAction::Raw { content_type }) => RouteResult::SendFile(FileToSend {
            path: path.to_path_buf(),
            content_type,
            len: meta.len(),
        }),
        Some(TypeAction::Code) => RouteResult::Response(serve_file_code(path, is_head)),
        Some(TypeAction::Markdown) => RouteResult::Response(serve_file_markdown(path, is_head)),
        None => {
            let content_type = guess_content_type(path);
            RouteResult::SendFile(FileToSend {
                path: path.to_path_buf(),
                content_type,
                len: meta.len(),
            })
        }
    }
}

// Note: raw file responses are streamed from disk in `connection.rs`.

fn serve_file_code(path: &Path, is_head: bool) -> Response {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            return Response::html(404, "Not Found", "<h1>404 Not Found</h1>")
                .without_body_if(is_head);
        }
    };

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    let title = format!(
        "{} (code view)",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let text = String::from_utf8_lossy(&bytes);
    let html = render::render_code_page(&title, ext.as_deref(), &text);
    let resp = Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", "text/html; charset=utf-8");
    if is_head {
        resp
    } else {
        resp.with_body_bytes(html)
    }
}

fn serve_file_markdown(path: &Path, is_head: bool) -> Response {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            return Response::html(404, "Not Found", "<h1>404 Not Found</h1>")
                .without_body_if(is_head);
        }
    };

    let title = format!(
        "{} (markdown)",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let text = String::from_utf8_lossy(&bytes);
    let html = render::render_markdown_page(&title, &text);
    let resp = Response::new()
        .with_status(200, "OK")
        .with_header("Content-Type", "text/html; charset=utf-8");
    if is_head {
        resp
    } else {
        resp.with_body_bytes(html)
    }
}

fn directory_listing_response(dir: &Path, raw_url_path: &str, decoded_url_path: &str) -> Response {
    let mut entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(_) => return Response::html(404, "Not Found", "<h1>404 Not Found</h1>"),
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

    let title = format!(
        "Directory listing for {}",
        url::html_escape(decoded_url_path)
    );
    let mut body = String::new();
    body.push_str("<style>body{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif} table{border-collapse:collapse} td{padding:4px 10px} a{text-decoration:none} a:hover{text-decoration:underline} .muted{color:#666} .upload{margin:12px 0;padding:12px;border:1px solid #e5e7eb;border-radius:8px;background:#fafafa} .upload button{padding:6px 10px}</style>");
    body.push_str(&format!("<h1>{}</h1>", title));
    body.push_str(&format!(
        "<div class=\"upload\" id=\"upload\" data-base=\"{base}\">\
<div><strong>Upload</strong> (multipart/form-data)</div>\
<div style=\"margin-top:8px\">\
<input type=\"file\" id=\"upload_files\" multiple />\
<button type=\"button\" id=\"upload_btn\">Upload</button>\
</div>\
<div class=\"muted\" id=\"upload_status\" style=\"margin-top:8px\"></div>\
</div>\
<script>\
(function(){{\
const root=document.getElementById('upload');\
const base=root.dataset.base; \
const input=document.getElementById('upload_files');\
const btn=document.getElementById('upload_btn');\
const status=document.getElementById('upload_status');\
btn.addEventListener('click', async () => {{\
  const files = Array.from(input.files||[]); \
  if(files.length===0){{status.textContent='No files selected.';return;}}\
  btn.disabled=true; \
  try {{\
    const form = new FormData(); \
    for(const f of files){{\
      status.textContent = 'Uploading ' + f.name + ' (' + f.size + ' bytes)...'; \
      form.append('file', f, f.name); \
    }}\
    const resp = await fetch(base, {{method:'POST', body:form}}); \
      if(!resp.ok){{throw new Error('Upload failed: ' + resp.status + ' ' + resp.statusText);}}\
    status.textContent='Upload complete. Refreshing...'; \
    setTimeout(() => location.reload(), 300); \
  }} catch(e) {{\
    status.textContent = String(e); \
  }} finally {{\
    btn.disabled=false; \
  }}\
}});\
}})();\
</script>",
        base = url::html_escape(&ensure_trailing_slash_owned(raw_url_path)),
    ));
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
            format!("{}/", url::html_escape(&name))
        } else {
            url::html_escape(&name)
        };

        let href = if is_dir {
            format!("{}{}/", href_prefix, url::url_escape_path_component(&name))
        } else {
            format!("{}{}", href_prefix, url::url_escape_path_component(&name))
        };

        let size = if is_dir {
            "-".to_string()
        } else {
            ent.metadata()
                .map(|m| human_size_bytes(m.len()))
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
        .with_body_bytes(
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body>{}</body></html>",
                title, body
            )
            .into_bytes(),
        )
}

fn ensure_trailing_slash_owned(url_path: &str) -> String {
    if url_path.ends_with('/') {
        url_path.to_string()
    } else {
        format!("{}/", url_path)
    }
}

fn guess_content_type_builtin(path: &Path) -> &'static str {
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
        "rs" | "py" | "go" | "log" | "md" | "toml" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn guess_content_type(path: &Path) -> String {
    let builtin = guess_content_type_builtin(path);
    if builtin != "application/octet-stream" {
        return builtin.to_string();
    }

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some(info) = minimime::lookup_by_filename(file_name) {
        let is_binary = info.is_binary();
        let mut ct = info.content_type;
        if !ct.contains("charset=")
            && !is_binary
            && (ct.starts_with("text/")
                || ct.eq_ignore_ascii_case("application/javascript")
                || ct.eq_ignore_ascii_case("application/json"))
        {
            ct.push_str("; charset=utf-8");
        }
        return ct;
    }
    guess_content_type_builtin(path).to_string()
}

fn human_size_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if size >= 10.0 {
        format!("{:.0} {}", size, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

pub(crate) trait ResponseHeadExt {
    fn without_body_if(self, is_head: bool) -> Self;
}

impl ResponseHeadExt for Response {
    fn without_body_if(self, is_head: bool) -> Self {
        if is_head { self.without_body() } else { self }
    }
}
