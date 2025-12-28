
mod method;
mod content_type;
mod header;

use std::io::{BufRead, Cursor};

pub use method::*;
pub use header::*;
pub use content_type::*;

pub fn find_empty_line_index(content: &str) -> (usize, usize) {
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
