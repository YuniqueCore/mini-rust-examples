use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    str::FromStr,
};

use sarge::{ArgumentType, prelude::*};

use crate::impl_deref_mut;

sarge! {
    #[derive(Debug)]
    pub Args,

    /// The bind addr for serving.
    #ok 'l' @HTTPOXY_BIND pub bind: BindAddr = BindAddr::from_str("127.0.0.1:8081").unwrap(),

    /// the dir/file will be served
    #ok 'r' @HTTPOXY_REVERSE pub reverse: BindAddr = BindAddr::from_str("127.0.0.1:8082").unwrap(),

    /// log level: "" means no log, v - info, vv - debug, vvv - trace
    #ok 'v' @HTTPOXY_LOG_LEVEL pub log_level:LogLevel = LogLevel("info".into()),

    /// log with color?
    #ok pub colored:bool = false,

    /// help
    #ok 'h' pub help: bool = false,
}

#[derive(Debug)]
pub struct LogLevel(String);

impl FromStr for LogLevel {
    type Err = core::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(String::from_str(s)?))
    }
}

impl ArgumentType for LogLevel {
    type Error = ArgParseError;
    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        const VERBOSE_PAT: char = 'v';

        if let Some(v) = val {
            let level_str = match v.trim().to_ascii_lowercase().as_str() {
                "off" => "off",
                "err" | "error" => "error",
                "warn" | "warning" => "warn",
                "info" => "info",
                "debug" => "debug",
                "trace" => "trace",
                s => {
                    let count = s
                        .chars()
                        .filter(|c| c.eq_ignore_ascii_case(&VERBOSE_PAT))
                        .count();
                    match count {
                        0 => "off",
                        1 => "info",
                        2 => "debug",
                        3 => "trace",
                        _ => "trace",
                    }
                }
            };

            return Ok(LogLevel(level_str.into())).into();
        }

        Ok(LogLevel("info".into())).into()
    }
}

impl_deref_mut!(LogLevel(String));

#[derive(Debug)]
pub struct BindAddr(SocketAddr);

impl ArgumentType for BindAddr {
    type Error = ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        if let Some(v) = val {
            let bind_addr = SocketAddr::from_str(v).ok()?;
            return Ok(BindAddr(bind_addr)).into();
        }
        None
    }
}

impl FromStr for BindAddr {
    type Err = std::net::AddrParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(SocketAddr::from_str(s)?))
    }
}

impl_deref_mut!(BindAddr(SocketAddr));
