pub fn percent_decode_path(path: &str) -> std::result::Result<String, ()> {
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(());
                }
                let hi = from_hex(bytes[i + 1]).ok_or(())?;
                let lo = from_hex(bytes[i + 2]).ok_or(())?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    std::str::from_utf8(&out)
        .map(|s| s.to_string())
        .map_err(|_| ())
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn url_escape_path_component(s: &str) -> String {
    // Minimal percent-encoding for path components.
    // Keep unreserved per RFC 3986: ALPHA / DIGIT / "-" / "." / "_" / "~"
    let mut out = String::new();
    for &b in s.as_bytes() {
        let is_unreserved =
            matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_decode_path() {
        assert_eq!(percent_decode_path("/a%20b").unwrap(), "/a b");
        assert_eq!(percent_decode_path("/%7Euser").unwrap(), "/~user");
        assert!(percent_decode_path("/%2").is_err());
        assert!(percent_decode_path("/%ZZ").is_err());
    }

    #[test]
    fn test_url_escape_path_component() {
        assert_eq!(url_escape_path_component("a b"), "a%20b");
        assert_eq!(url_escape_path_component("a/b"), "a%2Fb");
        assert_eq!(url_escape_path_component("~_-.a"), "~_-.a");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<a&\"'>"), "&lt;a&amp;&quot;&#39;&gt;");
    }
}
