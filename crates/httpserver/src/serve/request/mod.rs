use std::str::FromStr;

use anyhow::Result;

use crate::serve::{Header, Method, request::request_line::RequestLine};

mod request_line;

#[derive(Debug)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub headers: Vec<Header>, // 保持顺序，简单好用
    pub body: Option<Vec<u8>>,
}

impl Request {
    pub fn parse_head(head: &str) -> Result<(Self, Option<usize>)> {
        let mut lines = head.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("failed to get the request line"))?
            .trim();

        let (method, path, _version) = RequestLine::from_str(request_line)?.split();

        let mut headers = Vec::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            if line.contains(':') {
                headers.push(Header::from_str(line.trim())?);
            }
        }

        let content_length = headers
            .iter()
            .find(|h| h.key_eq_ignore_ascii_case("Content-Length"))
            .and_then(|h| h.value.parse::<usize>().ok());

        Ok((
            Self {
                method,
                path,
                headers,
                body: None,
            },
            content_length,
        ))
    }

    pub fn with_body_bytes(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key_eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}
