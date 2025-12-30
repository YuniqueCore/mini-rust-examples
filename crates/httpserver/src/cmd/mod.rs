use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    path::PathBuf,
    str::FromStr,
};

use sarge::{ArgumentType, prelude::*};

use crate::impl_deref_mut;
use crate::serve::auth::{AuthParseError, BasicAuth};
use crate::serve::types::{TypeMappings, TypeSpecParseError};

sarge! {
    #[derive(Debug)]
    pub Args,

    /// The bind addr for serving.
    #ok 'l' pub bind:BindAddr = BindAddr::from_str("127.0.0.1:8080").unwrap(),

    /// the dir/file will be served
    #ok 's' pub serve: ServePath = ServePath::from_str("public").unwrap(),

    /// log level: "" means no log, v - info, vv - debug, vvv - trace
    #ok 'v' pub log_level:LogLevel = LogLevel("info".into()),

    /// log with color?
    #ok pub colored:bool = false,

    /// Content-type & render mappings, e.g. -t "rs|toml=code;md=html;log=text"
    #ok 't' pub types: TypeMappingsArg = TypeMappingsArg(TypeMappings::parse_spec("rs|toml=code;md=html").unwrap()),

    /// Optional basic auth in the form "name@password" (enables HTTP Basic auth).
    #ok 'a' pub auth: AuthArg = AuthArg::default(),

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

#[derive(Debug)]
pub struct ServePath(PathBuf);

impl ArgumentType for ServePath {
    type Error = ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        if let Some(v) = val {
            let path = PathBuf::from_str(v).unwrap();
            if !path.exists() {
                return None;
            }
            return Ok(ServePath(path)).into();
        }
        None
    }
}

impl FromStr for ServePath {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(PathBuf::from_str(s)?))
    }
}

impl_deref_mut!(ServePath(PathBuf));

#[derive(Debug, Clone,Default)]
pub struct TypeMappingsArg(TypeMappings);

impl ArgumentType for TypeMappingsArg {
    type Error = TypeSpecParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        match val {
            None => Some(Ok(Self::default())),
            Some(v) => Some(TypeMappings::parse_spec(v).map(TypeMappingsArg)),
        }
    }

    fn help_default_value(value: &Self) -> Option<String> {
        value.0.default_value()
    }
}

impl From<TypeMappingsArg> for TypeMappings {
    fn from(value: TypeMappingsArg) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthArg(Option<BasicAuth>);

impl ArgumentType for AuthArg {
    type Error = AuthParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        match val {
            None => Some(Ok(Self::default())),
            Some(v) => Some(BasicAuth::parse_user_at_password(v).map(|a| AuthArg(Some(a)))),
        }
    }

    fn help_default_value(value: &Self) -> Option<String> {
        Some(format!("{:?}",value.0)) 
    }
}

impl From<AuthArg> for Option<BasicAuth> {
    fn from(value: AuthArg) -> Self {
        value.0
    }
}
