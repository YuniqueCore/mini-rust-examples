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
use std::{fmt::Debug, io::Write, net::SocketAddr, thread};

use paste::paste;
use sarge::prelude::*;
// use smol::prelude::*;

sarge! {
    Args,

    // socket addr
    //
    // deafult :127.0.0.1:9912
    > "help"
    #ok 's' socket_addr: String ,
    #ok 't' target_addr: String,
    #err 'h' help:bool =true,
}

impl Debug for Args {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Args")
            .field("socket_addr", &self.socket_addr)
            .field("target_addr", &self.target_addr)
            .field("help", &self.help)
            .finish()
    }
}

const HTTP_VERSION: &str = "HTTP/1.1";

#[derive(Debug)]
enum ResponseStatusCode {
    TwoXX(String),
    ThreeXX(String),
    FourXX(String),
    FiveXX(String),
}

macro_rules! define_it {
    // macro! for enum
    (
        $( #[$attr_meta:meta] )+
        $v:vis enum $name:ident {
           $last_ident:ident,
           $(
                $idents:ident
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )+
        $v enum $name{
            $(
                $idents ,
            )*
            $last_ident
        }

        impl ::core::fmt::Display for $name{
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let value = match self {
                        $(
                            Self:: $idents =>  stringify!($idents),
                        )*
                        Self:: $last_ident =>  stringify!($last_ident)
                    };
                    write!(f, "{}", value)
            }
        }

        paste!{
            impl $name {
                pub const ITEMS_COUNT: u32 = Self::$last_ident as u32 + 1;
                pub const [<ITEMS_ $name:upper>]: [Self; Self::ITEMS_COUNT as usize] = [
                   $(
                        Self:: $idents ,
                    )*
                    Self:: $last_ident,
                ];
            }
        }
    };

    // macro! for struct
   (
        $( #[$attr_meta:meta] )+
        $v:vis struct $name:ident {
           $(
              $vv:vis  $idents:ident: $idents_ty:ty = $default_val:expr
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )+
        $v struct $name{
            $(
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
        PUT,
        GET,
        POST,
        DELETE,
    }
);

pub struct ReqBuilder {
    req: String,
}

macro_rules! impl_req_method {
    ($method:ident) => {
        impl ReqBuilder {
            paste! {
                pub fn [< $method:lower >](self, route: &str) -> Self {
                    use ReqMethod::*;
                    self.req_method($method, route)
                }
            }
        }
    };
    ($ty:ident :: $variant:ident) => {
        impl ReqBuilder {
            paste! {
                pub fn [< $variant:lower >](self, route: &str) -> Self {
                    self.req_method($ty::$variant, route)
                }
            }
        }
    };
    ($method:path => $fn_name:ident) => {
        impl ReqBuilder {
            pub fn $fn_name(self, route: &str) -> Self {
                self.req_method($method, route)
            }
        }
    };
}

impl_req_method!(crate::ReqMethod::GET => get);
impl_req_method!(ReqMethod::POST);
impl_req_method!(DELETE);
impl_req_method!(PUT);

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
        self
    }

    pub fn append_headers<I, S>(mut self, headers: I) -> Self
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

        self
    }

    pub fn append_data(mut self, data: &str) -> Self {
        self.req.push_str(data);

        self
    }

    pub fn build(self) -> String {
        self.req
    }
}

fn main() -> anyhow::Result<()> {
    use std::net::TcpStream;
    use std::str::FromStr;
    let (args, mut remainder) = Args::parse()?;
    if args.help.ok().is_some_and(|b| b) {
        Args::print_help();
        return Ok(());
    }

    remainder.remove(0); // remove the executable path

    println!("{args:#?}\n{remainder:?}");
    // let bind_socket = SocketAddr::from_str(&args.socket_addr)?;
    let target_socket = SocketAddr::from_str(&args.target_addr.unwrap())?;

    let mut tcp_stream = TcpStream::connect(target_socket)?;

    let mut content = remainder.join("\n");

    println!("{content}");

    let send_task = thread::spawn(move || {
        let res = tcp_stream.write_all(unsafe { content.as_bytes_mut() });
        let _ = dbg!(res);
        std::thread::sleep(std::time::Duration::from_secs(2));
    });

    send_task
        .join()
        .expect("should be successfully write the data");
    Ok(())
}
