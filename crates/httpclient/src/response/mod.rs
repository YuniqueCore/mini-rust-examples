mod status_code;
mod status_line;

use std::{io, str::FromStr};

#[allow(unused_imports)]
pub use status_code::*;
pub use status_line::*;

use crate::common::Header;

#[derive(Debug, Default)]
pub struct Resp {
    status: StatusLine,
    headers: Vec<Header>,
    data: Option<String>,
    raw_resp: String,
}

impl Resp {
    pub fn resp(mut self, response_raw_str: &str) -> Self {
        self.raw_resp = response_raw_str.into();
        self
    }

    pub fn parse(mut self) -> Result<Self, io::Error> {
        let mut lines = self.raw_resp.lines();
        let status_line = lines.next().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Error: invalid data, no status line found"),
        ))?;
        if status_line.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Error: empty response (server closed connection or protocol mismatch)".to_string(),
            ));
        }
        self.status = StatusLine::from_str(status_line)?;

        let mut headers = Vec::new();
        while let Some(header) = lines.next() {
            let header = header.trim();
            if !header.is_empty() {
                headers.push(header);
            } else {
                break;
            }
        }

        // WANRING!: flatten will collect all OK/Some while ignoring the err/None..
        // Should popagate the err/None outside instead of silently discard!
        self.headers = headers
            .into_iter()
            .map(Header::from_str)
            .flatten()
            .collect::<Vec<_>>();

        let mut data = Vec::new();
        while let Some(d) = lines.next() {
            data.push(d);
        }

        self.data = if data.len() > 0 {
            Some(data.join("\n"))
        } else {
            None
        };

        Ok(self)
    }
}
