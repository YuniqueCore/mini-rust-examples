use crate::error::Result;

use std::net::IpAddr;

/// 127.0.0.1       localhost
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Host(IpAddr, String);

impl Host {
    pub fn new(ip: &str, domain: &str) -> Result<Self> {
        Ok(Host(ip.parse()?, domain.trim().into()))
    }

    pub fn ip(&self) -> &IpAddr {
        &self.0
    }

    pub fn domain(&self) -> &str {
        &self.1
    }
}
