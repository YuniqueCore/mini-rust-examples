use crate::serve::{Response, request::Request};

#[derive(Clone, Debug)]
pub struct BasicAuth {
    expected_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthParseError {
    pub message: String,
}

impl std::fmt::Display for AuthParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid --auth value: {}", self.message)
    }
}

impl std::error::Error for AuthParseError {}

impl BasicAuth {
    pub fn parse_user_at_password(spec: &str) -> Result<Self, AuthParseError> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err(AuthParseError {
                message: "empty string".to_string(),
            });
        }

        let (user, pass) = trimmed.split_once('@').ok_or_else(|| AuthParseError {
            message: "expected \"name@password\"".to_string(),
        })?;
        if user.is_empty() {
            return Err(AuthParseError {
                message: "missing name before '@'".to_string(),
            });
        }
        if pass.is_empty() {
            return Err(AuthParseError {
                message: "missing password after '@'".to_string(),
            });
        }

        let token = base64_encode(format!("{user}:{pass}").as_bytes());
        Ok(Self {
            expected_token: token,
        })
    }

    pub fn is_authorized(&self, req: &Request) -> bool {
        let Some(value) = req.header_value("Authorization") else {
            return false;
        };
        let mut parts = value.split_whitespace();
        let Some(scheme) = parts.next() else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case("Basic") {
            return false;
        }
        let Some(token) = parts.next() else {
            return false;
        };
        token == self.expected_token
    }

    pub fn unauthorized_response(&self) -> Response {
        Response::plain_text(
            401,
            "Unauthorized",
            "Unauthorized: missing or invalid credentials.\n",
        )
        .with_header("WWW-Authenticate", "Basic realm=\"httpserver\"")
    }
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0usize;
    while i < input.len() {
        let b0 = input[i];
        let b1 = input.get(i + 1).copied().unwrap_or(0);
        let b2 = input.get(i + 2).copied().unwrap_or(0);

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);

        if i + 1 < input.len() {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }

        if i + 2 < input.len() {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::{Header, Method, request::Request};

    #[test]
    fn test_parse_and_check() {
        let auth = BasicAuth::parse_user_at_password("u@p").unwrap();
        let req = Request {
            method: Method::GET,
            path: "/".to_string(),
            headers: vec![Header::new("Authorization", "Basic dTpw")],
            body: None,
        };
        assert!(auth.is_authorized(&req));
    }
}
