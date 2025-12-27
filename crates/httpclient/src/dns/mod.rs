use std::{
    net::{IpAddr, SocketAddr},
    net::ToSocketAddrs,
    str::FromStr,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::SystemTime,
    time::Duration,
};

mod cache;
mod host;
mod local;
mod remote;

pub use cache::*;
pub use host::*;
pub use local::*;
pub use remote::*;

#[derive(Debug)]
pub struct DnsResolver {
    cache: Arc<Mutex<DnsCache>>,
    cache_check_time: Duration,
    cache_handle: Option<JoinHandle<()>>,
    local_dns_resolver: LocalDnsResolver,
    remote_dns_resolver: RemoteDnsResolver,

    servers: Vec<String>,
    timeoout: Duration,
    retry: u8,
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self {
            cache: Arc::new(Mutex::new(DnsCache::new())),
            cache_check_time: Duration::from_secs(1),
            cache_handle: None,
            local_dns_resolver: LocalDnsResolver::new(Option::<String>::None),
            remote_dns_resolver: RemoteDnsResolver::new(Option::<Vec<_>>::None),
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

    pub fn local_resolver(mut self, local_resolver: LocalDnsResolver) -> Self {
        self.local_dns_resolver = local_resolver;
        self
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
        self.retry = times;
        self
    }

    pub fn start_cache_monitor(mut self) -> Self {
        let check_time = self.cache_check_time;

        let dns_cache = self.cache.clone();
        let clean_handler = std::thread::spawn(move || {
            loop {
                let now = SystemTime::now();
                {
                    let mut dns_cache_lock =
                        dns_cache.lock().unwrap_or_else(|poison| poison.into_inner());

                    let duration = dns_cache_lock.duration;
                    let expired: Vec<String> = dns_cache_lock
                        .liveness
                        .iter()
                        .filter_map(|(domain, ctime)| match now.duration_since(*ctime) {
                            Ok(elapsed) if elapsed > duration => Some(domain.clone()),
                            _ => None,
                        })
                        .collect();

                    for domain in expired {
                        dns_cache_lock.remove(&domain);
                    }
                }
                std::thread::sleep(check_time);
            }
        });

        self.cache_handle = Some(clean_handler);
        self
    }

    fn __local_resolve(&self, domain: &str) -> Option<&IpAddr> {
        self.local_dns_resolver.resolve(domain)
    }
    // TODO:
    async fn __remote_solve(&self, domain: &str) -> Option<Vec<IpAddr>> {
        self.remote_dns_resolver.resolve(domain).await
    }

    pub async fn resolve(&self, socket_addr: &str) -> Option<SocketAddr> {
        if let Ok(addr) = SocketAddr::from_str(socket_addr) {
            return Some(addr);
        }

        let (domain, port) = socket_addr.split_once(':')?;
        let port: u16 = port.parse().ok()?;

        // 1) cache
        if let Ok(cache) = self.cache.lock() {
            if let Some(ip) = cache.get(domain) {
                return Some(SocketAddr::new(*ip, port));
            }
        }

        // 2) local hosts file
        if let Some(ip) = self.__local_resolve(domain) {
            if let (Ok(mut cache), Ok(host)) = (self.cache.lock(), Host::new(&ip.to_string(), domain))
            {
                let _ = cache.insert(host);
            }
            return Some(SocketAddr::new(*ip, port));
        }

        // 3) system resolver (uses OS DNS config; often more reliable than hardcoded public DNS)
        if let Ok(mut addrs) = (domain, port).to_socket_addrs() {
            if let Some(addr) = addrs.next() {
                if let (Ok(mut cache), Ok(host)) =
                    (self.cache.lock(), Host::new(&addr.ip().to_string(), domain))
                {
                    let _ = cache.insert(host);
                }
                return Some(addr);
            }
        }

        // 3) remote DNS (do not hold cache locks across await)
        let ips = self.__remote_solve(domain).await?;
        let ip = ips.into_iter().next()?;
        if let (Ok(mut cache), Ok(host)) = (self.cache.lock(), Host::new(&ip.to_string(), domain)) {
            let _ = cache.insert(host);
        }
        return Some(SocketAddr::new(ip, port));

        // 4. fallback to None
        // None
    }
}
