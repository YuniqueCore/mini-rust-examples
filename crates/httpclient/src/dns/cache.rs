use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, SystemTime},
};

use crate::dns::Host;

#[derive(Debug)]
pub struct DnsCache {
    pub(crate) duration: Duration,
    // domain ip
    pub(crate) hosts: HashMap<String, IpAddr>,
    // domain ctime
    pub(crate) liveness: HashMap<String, SystemTime>,
}

impl Default for DnsCache {
    fn default() -> Self {
        Self {
            duration: Duration::from_mins(1),
            hosts: HashMap::new(),
            liveness: HashMap::new(),
        }
    }
}

impl DnsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn insert(&mut self, host: Host) -> Option<IpAddr> {
        let now = SystemTime::now();
        let old = self.hosts.insert(host.domain().into(), *host.ip());
        let _old_liveness = self.liveness.insert(host.domain().into(), now);
        old
    }

    pub fn get(&self, domain: &str) -> Option<&IpAddr> {
        self.hosts.get(domain)
    }

    pub fn remove(&mut self, domain: &str) -> Option<IpAddr> {
        let ip = self.hosts.remove(domain);
        let _ = self.liveness.remove(domain);
        ip
    }

    pub fn clear(&mut self) {
        self.hosts.clear();
        self.liveness.clear();
    }
}
