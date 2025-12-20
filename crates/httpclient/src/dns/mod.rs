use crate::error::Result;
use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
    time::Duration,
};

mod local;

pub use local::*;

/// 127.0.0.1       localhost
#[derive(Debug)]
pub struct Host(IpAddr, String);

impl Host {
    pub fn new(ip: &str, domain: &str) -> Result<Self> {
        Ok(Host(ip.parse()?, domain.into()))
    }

    pub fn ip(&self) -> &IpAddr {
        &self.0
    }

    pub fn domain(&self) -> &str {
        &self.1
    }
}

#[derive(Debug)]
pub struct DnsResolver {
    servers: Vec<String>,
    timeoout: Duration,
    retry: u8,
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self {
            servers: vec![],
            timeoout: Duration::from_secs(3),
            retry: 3,
        }
    }
}

impl DnsResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn servers(mut self, servers: impl Into<Vec<String>>) -> Self {
        self.servers = servers.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeoout = timeout;
        self
    }

    pub fn retry(mut self, times: u8) -> Self {
        self.retry == times;
        self
    }

    pub fn resolve(socket_addr: &str) -> Result<SocketAddr> {
        if Ok(addr) = SocketAddr::from_str(socket_addr) {
            return Ok(addr);
        }
    }
}
