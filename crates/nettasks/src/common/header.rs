use core::fmt;
use std::{io, str::FromStr};

#[derive(Debug)]
pub struct Header {
    key: String,
    value: String,
}

impl Header {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl FromStr for Header {
    type Err = std::io::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tuple: Vec<&str> = s.splitn(2, ':').map(str::trim).collect();
        if tuple.len() < 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "error"));
        }
        Ok(Header::new(tuple[0], tuple[1]))
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.value)
    }
}
