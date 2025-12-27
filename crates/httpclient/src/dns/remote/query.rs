use std::{
    future::Future,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, UdpSocket},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    thread,
    time::Duration,
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

#[derive(Debug, Clone)]
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

    fn query_once(&self, domain: &str, qtype: QType, server: SocketAddr) -> Option<Vec<IpAddr>> {
        let mut resp = Vec::new();
        let mut id = u16::MAX;
        let qtype = qtype.into();

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

    pub async fn query(
        &self,
        domain: &str,
        qtype: QType,
        server: SocketAddr,
    ) -> Option<Vec<IpAddr>> {
        let domain = domain.to_string();
        let client = self.clone();
        let state = Arc::new(Mutex::new(QueryState {
            result: None,
            waker: None,
        }));
        let state_for_thread = state.clone();

        thread::spawn(move || {
            let result = client.query_once(&domain, qtype, server);
            let waker = {
                let mut state = state_for_thread.lock().unwrap();
                state.result = Some(result);
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        });

        QueryFuture { state }.await
    }
}

struct QueryState {
    result: Option<Option<Vec<IpAddr>>>,
    waker: Option<Waker>,
}

struct QueryFuture {
    state: Arc<Mutex<QueryState>>,
}

impl Future for QueryFuture {
    type Output = Option<Vec<IpAddr>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.result.take() {
            return Poll::Ready(result);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
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
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
        thread,
        time::Duration,
    };

    use crate::{
        dns::remote::query::{DnsQueryClient, QType},
        error::Result,
    };

    fn block_on<F: Future>(future: F) -> F::Output {
        struct ThreadWake {
            thread: thread::Thread,
        }

        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.thread.unpark();
            }

            fn wake_by_ref(self: &Arc<Self>) {
                self.thread.unpark();
            }
        }

        let waker = Waker::from(Arc::new(ThreadWake {
            thread: thread::current(),
        }));
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park(),
            }
        }
    }

    struct Join2<F1, F2>
    where
        F1: Future,
        F2: Future,
    {
        f1: Pin<Box<F1>>,
        f2: Pin<Box<F2>>,
        out1: Option<F1::Output>,
        out2: Option<F2::Output>,
    }

    impl<F1, F2> Unpin for Join2<F1, F2>
    where
        F1: Future,
        F2: Future,
    {
    }

    fn join2<F1, F2>(f1: F1, f2: F2) -> Join2<F1, F2>
    where
        F1: Future,
        F2: Future,
    {
        Join2 {
            f1: Box::pin(f1),
            f2: Box::pin(f2),
            out1: None,
            out2: None,
        }
    }

    impl<F1, F2> Future for Join2<F1, F2>
    where
        F1: Future,
        F2: Future,
    {
        type Output = (F1::Output, F2::Output);

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.as_mut().get_mut();

            if this.out1.is_none() {
                if let Poll::Ready(v) = this.f1.as_mut().poll(cx) {
                    this.out1 = Some(v);
                }
            }

            if this.out2.is_none() {
                if let Poll::Ready(v) = this.f2.as_mut().poll(cx) {
                    this.out2 = Some(v);
                }
            }

            if this.out1.is_some() && this.out2.is_some() {
                return Poll::Ready((this.out1.take().unwrap(), this.out2.take().unwrap()));
            }

            Poll::Pending
        }
    }
    #[test]
    fn test_query() -> Result<()> {
        let dns_query_client = DnsQueryClient::new().with_timeout(Duration::from_secs(2));

        let server = "1.1.1.1:53".parse().unwrap();

        let rest = dns_query_client.query_once("google.com", QType::A, server);

        dbg!(rest);
        let rest = dns_query_client.query_once("google.com", QType::A, server);

        dbg!(rest);
        Ok(())
    }

    #[test]
    fn test_query_concurrent_block_on() -> Result<()> {
        let dns_query_client = DnsQueryClient::new().with_timeout(Duration::from_secs(2));
        let server = "1.1.1.1:53".parse().unwrap();

        let f1 = dns_query_client.query("example.com", QType::A, server);
        let f2 = dns_query_client.query("google.com", QType::A, server);

        let (r1, r2) = block_on(join2(f1, f2));

        println!("result1: {r1:?}");
        println!("result2: {r2:?}");
        Ok(())
    }
}
