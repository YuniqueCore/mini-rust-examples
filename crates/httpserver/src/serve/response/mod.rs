//! Response the http client request

use std::str::FromStr;

use anyhow::Result;

use crate::serve::{Header, common, response::status_line::StatusLine};

mod status_line;

const HTTP_VESION: &str = "http/1.1";

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
            version: HTTP_VESION.into(),
            status: 200,
            reason: "OK".into(),
            ..Default::default()
        }
    }
}

impl Response {
    pub fn new() -> Self {
        Self::default()
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

    pub fn build(&self) -> Result<String> {
        let mut response = String::new();

        response.push_str(&format!("{} {} {}", self.version, self.status, self.reason));
        for h in self.headers.iter() {
            response.push_str(&h.to_string());
        }

        if let Some(d) = &self.body {
            response.push_str(&String::from_utf8_lossy(&d));
        }

        Ok(response)
    }

    pub fn to_string(&self) -> Result<String> {
        self.build()
    }

    pub fn parse(response: &str) -> Result<Self> {
        let (char_idx, _line_idx) = common::find_empty_line_index(response);
        let (meta, data) = response.split_at(char_idx);

        let mut meta_iter = meta.split("\r\n");
        let (version,status,reason) = StatusLine::from_str(
            meta_iter
                .next()
                .ok_or(anyhow::anyhow!("failed to get the status line"))?
                .trim(),
        )?.split();

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
