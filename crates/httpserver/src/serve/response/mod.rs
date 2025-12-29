//! Response the http client request

use std::str::FromStr;

use anyhow::Result;

use crate::serve::{Header, common, response::status_line::StatusLine};

mod status_line;

const HTTP_VERSION: &str = "HTTP/1.1";

const INTERNAL_ERROR: &str = r#"HTTP/1.1 500 Internal Server Error
Content-Type: text/html;
Content-Length: 123

<!doctype html>
<html lang="en">
<head>
  <title>500 Internal Server Error</title>
</head>
<body>
  <h1>Internal Server Error</h1>
  <p>The server was unable to complete your request. Please try again later.</p>
</body>
</html>
"#;

const CLIENT_REQUEST_ERROR: &str = r#"HTTP/1.1 401 Request Error
Content-Type: text/html;
Content-Length: 123

<!doctype html>
<html lang="en">
<head>
  <title>401 Request error</title>
</head>
<body>
  <h1>Client request Error</h1>
  <p>The server was unable to complete your request. Please try again later.</p>
</body>
</html>
"#;

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

    pub fn with_status_line(mut self, status_line: StatusLine) -> Self {
        let (version, status, reason) = status_line.split();
        self.version = version;
        self.status = status;
        self.reason = reason;
        self
    }

    pub fn with_headers(mut self, headers: &[Header]) -> Self {
        self.headers = headers.to_vec();
        self
    }

    pub fn with_body(mut self, body: &str) -> Self {
        self.body = Some(body.as_bytes().to_vec());
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

    pub fn build(&self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.to_bytes()).to_string())
    }

    pub fn to_string(&self) -> String {
        self.build().unwrap_or(String::from(INTERNAL_ERROR))
    }

    pub fn internal_error() -> &'static [u8] {
        INTERNAL_ERROR.as_bytes()
    }

    pub fn error_request() -> &'static [u8] {
        CLIENT_REQUEST_ERROR.as_bytes()
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

    pub fn parse(response: &str) -> Result<Self> {
        let (char_idx, _line_idx) = common::find_empty_line_index(response);
        let (meta, data) = response.split_at(char_idx);

        let mut meta_iter = meta.split("\r\n");
        let (version, status, reason) = StatusLine::from_str(
            meta_iter
                .next()
                .ok_or(anyhow::anyhow!("failed to get the status line"))?
                .trim(),
        )?
        .split();

        let mut headers = vec![];
        for i in meta_iter {
            if i.contains(':') {
                let header = i.trim();
                headers.push(Header::from_str(header)?);
            }
        }

        let data = data.trim();
        let body = if data.len() > 0 {
            Some(data.as_bytes().to_vec())
        } else {
            None
        };

        Ok(Self {
            version,
            status,
            reason,
            headers,
            body,
        })
    }
}

impl FromStr for Response {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(s)
    }
}
