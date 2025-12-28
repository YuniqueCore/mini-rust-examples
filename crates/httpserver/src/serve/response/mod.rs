//! Response the http client request

use std::{
    io::{BufRead, Cursor},
    str::FromStr,
};

use anyhow::Result;

use crate::serve::response::{header::Header, status_line::StatusLine};

mod content_type;
mod header;
mod status_line;

const HTTP_VESION: &str = "http/1.1";

#[derive(Debug)]
pub struct Response {
    status_line: StatusLine,
    headers: Vec<Header>,
    data: Option<String>,
}

impl Response {
    pub fn parse(response: &str) -> Result<Self> {
        let (char_idx, _line_idx) = find_empty_line_index(response);
        let (meta, data) = response.split_at(char_idx);

        let mut meta_iter = meta.split("\r\n");
        let status_line = StatusLine::from_str(
            meta_iter
                .next()
                .ok_or(anyhow::anyhow!("failed to get the status line"))?
                .trim(),
        )?;

        let mut headers = vec![];
        for i in meta_iter {
            if i.contains(':') {
                let header = i.trim();
                headers.push(Header::from_str(header)?);
            }
        }

        let data = data.trim();
        let data = if data.len()>0{ Some(data.to_owned()) } else{ None };

       Ok( Self {
            status_line,
            headers,
            data,
        })
    }
}

fn find_empty_line_index(content: &str) -> (usize, usize) {
    let mut cursor = Cursor::new(content);
    let (mut line_idx, mut char_idx) = (0, 0);
    let mut buf = String::new();
    while let Ok(len) = cursor.read_line(&mut buf) {
        print!("{} -> {}", len, buf);
        if len == 1 {
            break;
        }
        char_idx += len;
        line_idx += 1;
        buf.clear();
    }

    (char_idx, line_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_find_empty_line_idx() {
        let content = r#"0POST /api/v1/items HTTP/1.1
1Host: api.example.com
2User-Agent: example-client/1.0
3Content-Type: application/json
4Content-Length: 27
5Connection: close
6

8{"name":"book","qty":1}
        "#;

        let (char_idx, line_idx) = find_empty_line_index(content);

        assert_eq!(157, char_idx);
        assert_eq!(7, line_idx);
        let (meta, data) = content.split_at(char_idx);

        println!("{}", meta.trim());
        println!("{}", data.trim());
    }
}
