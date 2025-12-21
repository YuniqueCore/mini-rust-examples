use std::{net::IpAddr, time::Duration};

use crate::error::{Error, Result};

const QTYPE_A: u16 = 1;
const QTYPE_AAAA: u16 = 28;
const QCLASS_IN: u16 = 1;
const QNAME_MAX_LEN: u8 = 63;

#[derive(Debug)]
pub struct DnsQueryClient {
    timeout: Duration,
    retry: u8,
}

impl Default for DnsQueryClient {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(2),
            retry: 3,
        }
    }
}

impl DnsQueryClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_retry(mut self, retry: u8) -> Self {
        self.retry = retry;
        self
    }

    pub fn query(&self, domain: &str) -> Option<IpAddr> {
        None
    }
}

fn encode_qname(name: &str, buf: &mut Vec<u8>) -> Result<()> {
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        return Err(Error::addr("name is empty").into());
    }

    for label in name.split('.') {
        if label.len() > 63 {
            return Err(Error::addr(&format!("qname is too long: {}", QNAME_MAX_LEN)).into());
        }
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0);
    Ok(())
}

fn gen_id() -> u16 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    // 简易“随机”，足够 demo；生产建议 OS 随机源（但那通常要 crate）
    (n as u16) ^ ((n >> 16) as u16)
}

fn build_query(name: &str, qtype: u16) -> Result<(u16, Vec<u8>), ()> {
    let id = gen_id();
    let mut pkt = Vec::with_capacity(512);

    // Header 12 bytes
    pkt.extend_from_slice(&id.to_be_bytes());
    pkt.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: RD=1
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    // Question
    encode_qname(name, &mut pkt)?;
    pkt.extend_from_slice(&qtype.to_be_bytes());
    pkt.extend_from_slice(&QCLASS_IN.to_be_bytes());

    Ok((id, pkt))
}
