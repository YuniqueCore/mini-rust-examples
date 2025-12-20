use std::{io, str::FromStr};

use crate::response::status_code::RespStatusCode;

#[derive(Debug, Default)]
pub struct StatusLine {
    code: RespStatusCode,
    http_version: String,
}

impl FromStr for StatusLine {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let triplet: Vec<&str> = s.splitn(3, ' ').map(str::trim).collect();
        if triplet.len() < 3 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("status data not complete: {s}"),
            ));
        }
        let code = RespStatusCode::parse(triplet[1], triplet[2])?;

        Ok(Self {
            code,
            http_version: triplet[0].into(),
        })
    }
}
