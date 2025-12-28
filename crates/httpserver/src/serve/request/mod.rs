use crate::serve::{Header, Method};



#[derive(Debug)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub version: String,       // "HTTP/1.1"
    pub headers: Vec<Header>,  // 保持顺序，简单好用
    pub body: Vec<u8>,
    pub peer: std::net::SocketAddr,
}
