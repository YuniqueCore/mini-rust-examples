use crate::dns::Host;
use crate::error::Result;
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{BufRead, BufReader},
    net::IpAddr,
    path::PathBuf,
};

#[cfg(windows)]
const DEFAULT_HOSTS_PATH: &str = r"C:\Windows\System32\drivers\etc\hosts";
#[cfg(not(windows))]
const DEFAULT_HOSTS_PATH: &str = "/etc/hosts";

#[derive(Debug)]
pub struct LocalDnsResolver {
    /// domain ip
    hosts: HashMap<String, IpAddr>,
}

impl LocalDnsResolver {
    pub fn new(host_path: Option<impl Into<PathBuf>>) -> Self {
        if let Some(host_path) = host_path {
            let path = host_path.into();
            if path.exists()
                && let Ok(hosts) = parse_host(path)
            {
                return Self { hosts };
            }
        }

        if let Ok(hosts) = parse_host(DEFAULT_HOSTS_PATH.into()) {
            return Self { hosts };
        }

        Self {
            hosts: HashMap::new(),
        }
    }

    pub fn resolve(&self, domain: &str) -> Option<&IpAddr> {
        self.hosts.get(domain.trim())
    }
}

fn parse_host(host_path: PathBuf) -> Result<HashMap<String, IpAddr>> {
    let mut hosts = HashMap::new();
    let content = OpenOptions::new().read(true).open(host_path)?;
    let mut reader = BufReader::new(content);

    let mut line = String::new();
    while let Ok(num_bytes) = reader.read_line(&mut line) {
        if num_bytes == 0 {
            break;
        }

        if line.trim().starts_with('#') {
            continue;
        }

        let ip_host: Vec<&str> = line.split_whitespace().collect();
        if ip_host.len() < 2 {
            continue;
        }

        if let Ok(host) = Host::new(ip_host[0], ip_host[1]) {
            let _old = hosts.insert(host.domain().into(), *host.ip());
        }
    }

    Ok(hosts)
}
