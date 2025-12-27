use crate::dns::remote::query::DnsQueryClient;
use crate::dns::remote::query::QType;

use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;

mod query;
mod response;

const DEFAULT_DNS_SERVERS: &[&str] = &["8.8.8.8:53", "1.1.1.1:53"];

#[derive(Debug)]
pub struct RemoteDnsResolver {
    dns_servers: Vec<SocketAddr>,
    query_client: DnsQueryClient,
}

impl RemoteDnsResolver {
    pub fn new(servers: Option<Vec<String>>) -> Self {
        let mut dns_svrs: Vec<_> = vec![];
        if let Some(dns_servers) = servers {
            dns_svrs = dns_servers
                .into_iter()
                .map_while(|s| SocketAddr::from_str(&s).ok())
                .collect();
        }

        if dns_svrs.len() > 0 {
            Self {
                dns_servers: dns_svrs,
                query_client: DnsQueryClient::new(),
            }
        } else {
            Self {
                dns_servers: DEFAULT_DNS_SERVERS
                    .into_iter()
                    .map(|&s| s.parse().unwrap())
                    .collect(),
                query_client: DnsQueryClient::new(),
            }
        }
    }

    pub fn with_query_client(mut self, query_client: DnsQueryClient) -> Self {
        self.query_client = query_client;
        self
    }

    pub async fn resolve(&self, domain: &str) -> Option<Vec<IpAddr>> {
        let mut results = Vec::new();
        // TODO: impl with rayon for parallelize
        for server in self.dns_servers.iter() {
            if self.query(&mut results, domain, QType::A, server).await {
                continue;
            } else {
                let _ = self.query(&mut results, domain, QType::AAAA, server).await;
            }
        }

        if results.len() > 0 {
            Some(results)
        } else {
            None
        }
    }

    async fn query(
        &self,
        results: &mut Vec<IpAddr>,
        domain: &str,
        qtype: QType,
        server: &SocketAddr,
    ) -> bool {
        let res = self.query_client.query(domain, qtype, *server).await;
        if let Some(ipaddr) = res
            && ipaddr.len() > 0
        {
            results.extend_from_slice(&ipaddr);
            true
        } else {
            false
        }
    }
}
