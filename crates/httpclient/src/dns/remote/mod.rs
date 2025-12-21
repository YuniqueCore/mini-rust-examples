use crate::error::Result;

use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;

mod query;
mod response;

const DEFAULT_DNS_SERVERS: &[&str] = &["8.8.8.8:53", "1.1.1.1:53"];

#[derive(Debug)]
pub struct RemoteDnsResolver {
    dns_servers: Vec<String>,
}

impl RemoteDnsResolver {
    pub fn new(servers: Option<Vec<String>>) -> Self {
        if let Some(dns_servers) = servers {
            return Self { dns_servers };
        }

        Self {
            dns_servers: DEFAULT_DNS_SERVERS.into_iter().map(|&s| s.into()).collect(),
        }
    }

    pub fn resolve(&self, domain: &str) -> Option<&IpAddr> {
        None
    }
}

fn server_endpoint(server: &str) -> Result<SocketAddr> {
    if let Ok(ip_addr) = server.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip_addr, 53));
    }

    Ok(SocketAddr::from_str(server)?)
}
