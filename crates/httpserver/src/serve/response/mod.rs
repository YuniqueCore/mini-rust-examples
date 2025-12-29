//! Response the http client request

use crate::serve::Header;

const HTTP_VERSION: &str = "HTTP/1.1";

#[derive(Debug)]
pub struct Response {
    pub version: String, // "HTTP/1.1"
    pub status: u16,     // 200, 404...
    pub reason: String,  // "OK"
    pub headers: Vec<Header>,
    pub body: Option<Vec<u8>>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            version: HTTP_VERSION.into(),
            status: 404,
            reason: "Not Found".into(),
            headers: vec![],
            body: None,
        }
    }
}

impl Response {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn without_body(mut self) -> Self {
        self.body = None;
        self
    }

    pub fn with_status(mut self, status: u16, reason: impl Into<String>) -> Self {
        self.status = status;
        self.reason = reason.into();
        self
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header::new(key, value));
        self
    }

    pub fn with_body_bytes(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    fn has_header(&self, key: &str) -> bool {
        self.headers.iter().any(|h| h.key_eq_ignore_ascii_case(key))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let body_len = self.body.as_ref().map(|b| b.len()).unwrap_or(0);

        let mut head = String::new();
        head.push_str(&format!(
            "{} {} {}\r\n",
            self.version, self.status, self.reason
        ));

        if !self.has_header("Content-Length") {
            head.push_str(&Header::new("Content-Length", body_len.to_string()).to_string());
        }
        if !self.has_header("Connection") {
            head.push_str(&Header::new("Connection", "close").to_string());
        }

        for h in self.headers.iter() {
            head.push_str(&h.to_string());
        }
        head.push_str("\r\n");

        let mut out = Vec::with_capacity(head.len() + body_len);
        out.extend_from_slice(head.as_bytes());
        if let Some(body) = &self.body {
            out.extend_from_slice(body);
        }
        out
    }

    pub fn plain_text(status: u16, reason: &str, body: &str) -> Self {
        Response::new()
            .with_status(status, reason)
            .with_header("Content-Type", "text/plain; charset=utf-8")
            .with_body_bytes(body.as_bytes().to_vec())
    }

    pub fn html(status: u16, reason: &str, body: &str) -> Self {
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{status} {reason}</title></head><body>{body}</body></html>"
        );
        Response::new()
            .with_status(status, reason)
            .with_header("Content-Type", "text/html; charset=utf-8")
            .with_body_bytes(html.into_bytes())
    }

    pub fn redirect_301(location: &str) -> Self {
        let body = format!(
            "<p>Moved Permanently: <a href=\"{location}\">{location}</a></p>",
            location = location
        );
        Response::new()
            .with_status(301, "Moved Permanently")
            .with_header("Location", location)
            .with_header("Content-Type", "text/html; charset=utf-8")
            .with_body_bytes(
                format!(
                    "<!doctype html><html><head><meta charset=\"utf-8\"><title>301 Moved Permanently</title></head><body>{body}</body></html>",
                    body = body
                )
                .into_bytes(),
            )
    }
}
