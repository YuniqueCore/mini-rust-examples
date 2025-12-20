use std::collections::HashMap;
use std::net::IpAddr;

const DEFAULT_HOSTS_PATH: &str = "/etc/hosts";

#[derive(Debug)]
pub struct RemoteDnsResolver {
    /// domain ip
    hosts: HashMap<String, IpAddr>,
}

impl RemoteDnsResolver {
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
