use std::{
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, UdpSocket},
    time::Duration,
    u16,
};

use crate::{
    dns::remote::response,
    error::{Error, Result},
};

pub const QTYPE_A: u16 = 1;
pub const QTYPE_AAAA: u16 = 28;
pub const QCLASS_IN: u16 = 1;
pub const QNAME_MAX_LEN: u8 = 63;

#[derive(Debug, Clone)]
pub enum QType {
    A,
    AAAA,
}

impl From<QType> for u16 {
    fn from(value: QType) -> Self {
        match value {
            QType::A => QTYPE_A,
            QType::AAAA => QTYPE_AAAA,
        }
    }
}

#[derive(Debug)]
pub struct DnsQueryClient {
    bind_addr: SocketAddr,
    timeout: Duration,
    retry: u8,
}

impl Default for DnsQueryClient {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:0".parse().unwrap(),
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

    fn udp_query(
        &self,
        name: &str,
        qtype: u16,
        server: SocketAddr,
    ) -> Result<(Vec<u8>, u16, bool)> {
        let (id, query) = build_query(name, qtype)?;

        let udp_socket = UdpSocket::bind(self.bind_addr)?;
        udp_socket.set_read_timeout(Some(self.timeout)).ok();
        udp_socket.send_to(&query, server)?;

        let mut buf = [0u8; 2048];
        let (n, _from) = udp_socket.recv_from(&mut buf)?;
        let resp = &buf[..n];

        // 如果 TC=1，需要 TCP
        // let (_ips, _ttl, tc) = parse_response(resp, id, qtype)?;
        let tc = false;
        Ok((resp.to_vec(), id, tc))
    }

    fn tcp_query(&self, name: &str, qtype: u16, server: SocketAddr) -> Result<(Vec<u8>, u16)> {
        let (id, query) = build_query(name, qtype)?;

        let timeout = self.timeout;

        // TCP: 前两字节长度
        let mut stream = TcpStream::connect(server)?;
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();

        let len = (query.len() as u16).to_be_bytes();
        stream.write_all(&len)?;
        stream.write_all(&query)?;

        let mut lbuf = [0u8; 2];
        stream.read_exact(&mut lbuf)?;
        let rlen = u16::from_be_bytes(lbuf) as usize;

        let mut rbuf = vec![0u8; rlen];
        stream.read_exact(&mut rbuf)?;
        Ok((rbuf, id))
    }

    pub fn query(&self, domain: &str, qtype: QType, server: SocketAddr) -> Option<Vec<IpAddr>> {
        let mut resp = Vec::new();
        let mut id = u16::MAX;
        let qtype: u16 = qtype.into();

        if let Ok((udp_resp, udp_id, tc)) = self.udp_query(domain, qtype, server) {
            if tc {
                let (tcp_resp, tcp_id) = self.tcp_query(domain, qtype, server).ok()?;
                resp = tcp_resp;
                id = tcp_id;
            } else {
                resp = udp_resp;
                id = udp_id;
            }
        }

        let (ips, _min_ttl, _tc) = response::parse(&resp, id, qtype).ok()?;

        Some(ips)
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
    // 简易“随机”
    (n as u16) ^ ((n >> 16) as u16)
}

fn build_query(name: &str, qtype: u16) -> Result<(u16, Vec<u8>)> {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        dns::remote::query::{DnsQueryClient, QType},
        error::Result,
    };
    #[test]
    fn test_query() -> Result<()> {
        let dns_query_client = DnsQueryClient::new().with_timeout(Duration::from_secs(2));

        let server = "1.1.1.1:53".parse().unwrap();

        let rest = dns_query_client.query("google.com", QType::A, server);

        dbg!(rest);
        Ok(())
    }
}
