use core::{error, str};
/// 任务 1：最小 HTTP 客户端
/// 目标
/// 理解 TCP 连接 + HTTP/1.1 请求/响应基本格式。
///
/// 实现要求
/// 使用 TCP socket 连接任意 HTTP 服务器（如 example.com:80）。
/// 手工构造并发送 HTTP/1.1 请求（至少 GET）：
/// 必须包含：GET /path HTTP/1.1、Host、Connection: close。
/// 读取完整响应：
/// 打印状态行、所有响应头；
/// 打印前 1024 字节响应体。
/// 对 HTTP 响应做最基础解析：
/// 提取 status code；
/// 解析部分常用头（Content-Length / Content-Type）。
///
/// 验收标准
/// 对多个网站（至少 3 个）能成功发请求并打印响应。
/// 响应头解析正确（status / Content-Type 等能正确显示）。
/// 程序异常处理合理：DNS 失败 / 连接失败不会崩溃，而是给出错误信息。
/// 建议反馈方式
/// 终端日志：清晰展示“连接 → 发送请求 → 接收 + 解析响应”的过程。
/// 可选：把解析结果以 JSON 打印，方便后续脚本检查
use std::{
    fmt::Debug,
    io::{self, BufReader, BufWriter, Read, Write},
    net::SocketAddr,
    ops::{Deref, DerefMut},
    str::FromStr,
    thread,
};

use pastey::paste;
use sarge::{ArgumentType, prelude::*};
// use smol::prelude::*;

#[derive(Debug)]
struct HeadersArg(Vec<String>);

impl ArgumentType for HeadersArg {
    type Error = ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        Some(Ok(Self(val?.split(';').map(|s| s.to_string()).collect())))
    }
}

impl Deref for HeadersArg {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HeadersArg {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

sarge! {
    #[derive(Debug)]
    Args,

    /// Legacy doc identifier was `>`; prefer Rust doc comments (`/// ...`).
    /// socket addr
    #ok 's' socket_addr: String = "127.0.0.1:9912" ,
    #ok 't' target_addr: String= "127.0.0.1:8000",
    #ok 'H' headers: HeadersArg,
    #ok 'd' data:Vec<String> = vec!["{'name': 'hello', 'data': 'world', 'age': 18 }"],
    #err 'h' help:bool = false,
}

const HTTP_VERSION: &str = "HTTP/1.1";

#[derive(Debug, Clone)]
enum RespStatusCode {
    TwoXX(String),
    ThreeXX(String),
    FourXX(String),
    FiveXX(String),
    Unknown(String),
}

macro_rules! impl_response_status_code {
    (
        $enum_parent:ident :: $enum_ident:ident
    ) => {
        paste! {
            impl $enum_parent {
                #[allow(unused)]
                pub fn [< $enum_ident:replace("XX", ""):lower >](msg: &str) -> Self {
                    $enum_parent::$enum_ident(msg.into())
                }
            }
        }
    };
}

impl_response_status_code!(RespStatusCode::TwoXX);
impl_response_status_code!(RespStatusCode::ThreeXX);
impl_response_status_code!(RespStatusCode::FourXX);
impl_response_status_code!(RespStatusCode::FiveXX);
impl_response_status_code!(RespStatusCode::Unknown);

impl Default for RespStatusCode {
    fn default() -> Self {
        Self::unknown("Unknown")
    }
}

impl RespStatusCode {
    pub fn parse(code: &str, msg: &str) -> Result<Self, io::Error> {
        let num: u16 = code.parse().map_err(io::Error::other)?;

        let msg = &format!("{num} {msg}");
        Ok(match num {
            n if n >= 600 => Self::unknown(msg),
            n if n >= 500 => Self::five(msg),
            n if n >= 400 => Self::four(msg),
            n if n >= 300 => Self::three(msg),
            n if n >= 200 => Self::two(msg),
            _ => Self::unknown(msg),
        })
    }
}

#[derive(Debug, Default)]
struct StatusLine {
    code: RespStatusCode,
    http_version: String,
}

impl FromStr for StatusLine {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let triplet: Vec<&str> = s.splitn(3, ' ').map(str::trim).collect();
        if triplet.len() < 3 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("status data not complete: {s}"),
            ));
        }
        let code = RespStatusCode::parse(triplet[1], triplet[2])?;

        Ok(Self {
            code,
            http_version: triplet[0].into(),
        })
    }
}

#[derive(Debug, Default)]
struct Resp {
    status: StatusLine,
    headers: Vec<String>,
    data: Option<String>,
    raw_resp: String,
}

impl Resp {
    pub fn resp(mut self, response_raw_str: &str) -> Self {
        self.raw_resp = response_raw_str.into();
        self
    }

    pub fn parse(mut self) -> Result<Self, io::Error> {
        let mut lines = self.raw_resp.split('\n');
        let status_line = lines.next().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Error: invalid data, no status line found"),
        ))?;
        self.status = StatusLine::from_str(status_line)?;

        let mut headers = Vec::new();
        while let Some(header) = lines.next() {
            let header = header.trim();
            if !header.is_empty() {
                headers.push(header);
            } else {
                break;
            }
        }

        self.headers = headers.into_iter().map(Into::into).collect();

        let mut data = Vec::new();
        while let Some(d) = lines.next() {
            data.push(d);
        }

        self.data = if data.len() > 0 {
            Some(data.join("\n"))
        } else {
            None
        };

        Ok(self)
    }
}

macro_rules! define_it {
    // macro! for enum
    (
        $( #[$attr_meta:meta] )*
        $v:vis enum $name:ident {
            $(
                $( #[$ident_attr_meta:meta] )*
                $idents:ident
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )*
        $v enum $name{
            $(
                $( #[$ident_attr_meta] )*
                $idents ,
            )*
        }

        impl $name {
            pub const ITEMS: &'static [Self] = &[
                $( Self::$idents, )*
            ];
            pub const ITEMS_COUNT: usize = Self::ITEMS.len();
        }

        paste! {
            macro_rules! [<with_variants_ $name>] {
                ($m:ident) => {
                    $m!($name; $( $idents ),*);
                };
            }
        }


        impl ::core::fmt::Display for $name{
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let value = match self {
                        $(
                            Self:: $idents =>  stringify!($idents),
                        )*
                    };
                    write!(f, "{}", value)
            }
        }
    };

    // macro! for struct
   (
        $( #[$attr_meta:meta] )*
        $v:vis struct $name:ident {
           $(
                $( #[$ident_attr_meta:meta] )*
                $vv:vis  $idents:ident: $idents_ty:ty = $default_val:expr
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )*
        $v struct $name{
            $(
                $( #[$ident_attr_meta] )*
                $idents: $idents_ty,
            )*
        }
        // TODO: impl default for $name

        impl ::core::fmt::Display for $name{
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let value = match self {
                        $(
                            Self:: $idents =>  stringify!($idents),
                        )*
                    };
                    write!(f, "{}", value)
            }
        }
    };
}

define_it!(
    /// nice to meet you
    #[derive(Debug)]
    pub enum ReqMethod {
        /// help
        PUT,
        GET,
        POST,
        DELETE,
    }
);

pub struct ReqBuilder {
    req: String,
}

macro_rules! impl_req_builder_methods {
    ($enum_ident:ident; $($variant:ident),* $(,)?) => {
        impl ReqBuilder {
            paste! {
                $(
                    pub fn [< $variant:lower >](self, route: &str) -> Self {
                        self.req_method($enum_ident::$variant, route)
                    }
                )*
            }
        }
    };
}

with_variants_ReqMethod!(impl_req_builder_methods);

impl ReqBuilder {
    pub fn new() -> Self {
        Self { req: String::new() }
    }
    /// Inner: Build the req method line like: GET /path HTTP/1.1
    fn __build_request_method(method: ReqMethod, route: &str, http_version: &str) -> String {
        format!("{} {} {}", method, route, http_version)
    }
    pub fn req_method(mut self, method: ReqMethod, route: &str) -> Self {
        self.req = Self::__build_request_method(method, route, HTTP_VERSION);
        self.req.push('\n');
        self
    }

    pub fn headers<I, S>(mut self, headers: I) -> Self
    where
        I: Iterator<Item = S>,
        S: AsRef<str>,
    {
        for h in headers {
            let h = h.as_ref();
            if !h.is_empty() {
                self.req.push_str(h);
                self.req.push('\n');
            }
        }
        self.req.push('\n');

        self
    }

    pub fn data(mut self, data: &str) -> Self {
        self.req.push_str(data);
        self.req.push('\n');

        self
    }

    pub fn build(self) -> String {
        self.req
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    use std::net::TcpStream;
    use std::str::FromStr;
    let (args, mut remainder) = Args::parse()?;
    if args.help.ok().is_some_and(|b| b) {
        let help = Args::help();
        println!("{help}");
        return Ok(());
    }

    remainder.remove(0); // remove the executable path

    println!("{args:#?}\n{remainder:?}\n\n");
    // let bind_socket = SocketAddr::from_str(&args.socket_addr)?;
    let target_socket = SocketAddr::from_str(&args.target_addr.unwrap())?;

    let tcp_stream = TcpStream::connect(target_socket)?;
    let tcp_tx = tcp_stream.try_clone()?;
    let buf_tx = BufWriter::new(tcp_tx);
    let buf_rx = BufReader::new(tcp_stream);

    let headers = if let Some(mut headers) = args.headers {
        headers.extend(remainder);
        headers
    } else {
        HeadersArg(remainder)
    };

    let data = &(if let Some(d) = args.data { d } else { vec![] }).join("\n");

    let req_content = ReqBuilder::new()
        .get("/abc")
        .headers(headers.iter())
        .data(data)
        .build();

    println!("\n\nrequest: \n{req_content}");

    let send_task = thread::spawn(move || {
        use std::net::Shutdown;
        let mut buf_tx = buf_tx;
        let res = buf_tx.write_all(req_content.as_bytes());
        if res.is_ok() {
            let _ = buf_tx.flush();
            let _ = buf_tx.get_ref().shutdown(Shutdown::Write);
        }
        let _ = dbg!(res);
    });

    let recv_task = thread::spawn(move || {
        let mut buf_rx = buf_rx;
        let mut buf = [0_u8; 2048];
        let mut response_vec: Vec<u8> = Vec::with_capacity(buf.len());
        loop {
            let res = buf_rx.read(&mut buf);
            match res {
                Ok(0) => {
                    let full_resp_str = String::from_utf8_lossy(&response_vec);
                    let resp = Resp::default().resp(&full_resp_str).parse();
                    match resp {
                        Ok(r) => {
                            dbg!(r);
                        }
                        Err(e) => {
                            eprintln!("{e}")
                        }
                    }
                    break;
                }
                Ok(len) => {
                    // TODO: should use correct encode/decode method to parse instead of uft8 default
                    let response = String::from_utf8_lossy(&buf[..len]);
                    response_vec.extend_from_slice(&buf[..len]);
                    println!("\n\nresponse: \n{response}\n\n");
                }
                Err(e) => {
                    eprintln!("Err: {e}");
                    continue;
                }
            }
        }
    });

    send_task
        .join()
        .expect("should be successfully write the data");

    recv_task
        .join()
        .expect("should be successfully read the data");

    Ok(())
}
