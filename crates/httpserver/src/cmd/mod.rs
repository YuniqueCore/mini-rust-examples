use std::{net::SocketAddr, ops::{Deref,DerefMut}, path::PathBuf, str::FromStr};

use sarge::{ArgumentType, prelude::*};

use crate::impl_deref_mut;


sarge! {
    #[derive(Debug)]
    pub Args,

    /// The bind addr for serving.
    #ok 'l' pub bind:BindAddr = BindAddr::from_str("127.0.0.1:8080").unwrap(),

    /// the dir/file will be served
    #ok 's' pub serve: ServePath = ServePath::from_str(".").unwrap(),

    /// help
    #ok 'h' pub help: bool = false,
}


#[derive(Debug)]
pub struct  BindAddr(SocketAddr);

impl ArgumentType for BindAddr {
    type Error=ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
       if let Some(v) = val {
            let bind_addr  = SocketAddr::from_str(v).ok()?;
            return Ok(BindAddr(bind_addr)).into();
       }
        None
    }
}

impl FromStr for  BindAddr {
    type Err=std::net::AddrParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
         Ok(Self(SocketAddr::from_str(s)?))
    }
}

impl_deref_mut!(BindAddr(SocketAddr));


#[derive(Debug)]
pub struct  ServePath(PathBuf);

impl ArgumentType for ServePath {
    type Error=ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
       if let Some(v) = val {
            let path  =PathBuf::from_str(v).unwrap();
            if !path.exists(){
                return  None;
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
