use std::path::Path;

use crate::serve::{
    Method, Response, render,
    request::Request,
    types::{TypeAction, TypeMappings},
    url,
};

pub fn serve_static(base: &Path, req: &Request, types: &TypeMappings) -> Response {
    match req.method {
        Method::GET | Method::HEAD => {}
        _ => {
            return Response::plain_text(405, "Method Not Allowed", "Method Not Allowed\n")
                .with_header("Allow", "GET, HEAD");
        }
    }

    let is_head = matches!(req.method, Method::HEAD);
    let raw_path = req.path.split('?').next().unwrap_or("/");
    let decoded_path = match url::percent_decode_path(raw_path) {
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
        if request_path != "/" && !request_path.ends_with('/') {
            let location = format!("{}/", raw_path);
            return Response::redirect_301(&location).without_body_if(is_head);
        }

        // Learning-friendly: always render a directory listing for directories.
        return directory_listing_response(&full, raw_path, request_path).without_body_if(is_head);
    }

    serve_file(&full, req, types)
}

fn serve_file(path: &Path, req: &Request, types: &TypeMappings) -> Response {
    let is_head = matches!(req.method, Method::HEAD);

    let action = types.action_for_path(path);
    match action {
        Some(TypeAction::Raw { content_type }) => serve_file_raw(path, &content_type, is_head),
        Some(TypeAction::Code) => serve_file_code(path, is_head),
        Some(TypeAction::Markdown) => serve_file_markdown(path, is_head),
        None => {
            let content_type = guess_content_type_builtin(path);
            serve_file_raw(path, content_type, is_head)
        }
    }
}

fn serve_file_raw(path: &Path, content_type: &str, is_head: bool) -> Response {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
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

pub(crate) trait ResponseHeadExt {
    fn without_body_if(self, is_head: bool) -> Self;
}

impl ResponseHeadExt for Response {
    fn without_body_if(self, is_head: bool) -> Self {
        if is_head { self.without_body() } else { self }
    }
}
