use std::str::FromStr;

use anyhow::Result;

use crate::serve::{Header, Method, common, request::request_line::RequestLine};

mod request_line;

#[derive(Debug)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub version: String,      // "HTTP/1.1"
    pub headers: Vec<Header>, // 保持顺序，简单好用
    pub body: Option<Vec<u8>>,
    pub peer: std::net::SocketAddr,
}

impl Request {
    pub fn parse(response: &str, peer: std::net::SocketAddr) -> Result<Self> {
        let (char_idx, _line_idx) = common::find_empty_line_index(response);
        let (meta, data) = response.split_at(char_idx);

        let mut meta_iter = meta.split("\r\n");
        let (method, path, version) = RequestLine::from_str(
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
            method,
            path,
            version,
            headers,
            body,
            peer,
        })
    }

    pub fn parse_head(head: &str, peer: std::net::SocketAddr) -> Result<(Self, Option<usize>)> {
        let mut lines = head.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("failed to get the request line"))?
            .trim();

        let (method, path, version) = RequestLine::from_str(request_line)?.split();

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
                version,
                headers,
                body: None,
                peer,
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
