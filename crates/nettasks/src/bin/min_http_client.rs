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
    env::args,
    fmt::Debug,
    io::Write,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    thread::{self, spawn},
};

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
