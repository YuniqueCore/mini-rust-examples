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
    future::Future,
    io::{BufReader, BufWriter, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

use crate::{
    cmd::{Args, HeadersArg}, dns::DnsResolver, request::ReqBuilder, response::Resp
};
// use smol::prelude::*;

mod cmd;
mod common;
mod r#const;
mod dns;
mod error;
mod request;
mod response;

use error::Result;

pub fn run() -> Result<()> {
    let (args, remainder) = cmd::parse()?;

    let target_socket = lookup_target(&args)?;

    let (buf_tx, buf_rx) = connect(target_socket)?;

    let (headers, data) = collect(args, remainder);

    let req_content = ReqBuilder::new()
        .get("/abc")
        .headers(headers.iter())
        .data(&data)
        .build();

    println!("\n\nrequest: \n{req_content}");

    let send_task = write(buf_tx, req_content);
    let recv_task = recv(buf_rx);

    send_task
        .join()
        .expect("should be successfully write the data");

    recv_task
        .join()
        .expect("should be successfully read the data");

    Ok(())
}

fn lookup_target(args: &Args) -> Result<SocketAddr> {
    use std::str::FromStr;
    let target_addr = args.target_addr.clone().unwrap();
    let mut target_socket = SocketAddr::from_str(&target_addr).map_err(error::Error::from_addr_parse_error);
    if target_socket.is_err(){
        let dns_client = DnsResolver::new();
        let socket_addr = block_on(dns_client.resolve(&target_addr));
        target_socket = socket_addr.ok_or(error::Error::addr(format!(
            "failed to resolve target addr: {target_addr}"
        )));
    }

   Ok(target_socket?)
}

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

fn collect(args: Args, remainder: Vec<String>) -> (HeadersArg, String) {
    let headers = if let Some(mut headers) = args.headers {
        headers.extend(remainder);
        headers
    } else {
        HeadersArg(remainder)
    };

    let data = (if let Some(d) = args.data { d } else { vec![] }).join("\n");
    (headers, data)
}

fn connect(
    target_socket: SocketAddr,
) -> Result<(
    BufWriter<std::net::TcpStream>,
    BufReader<std::net::TcpStream>,
)> {
    let tcp_stream = TcpStream::connect(target_socket)?;
    let tcp_tx = tcp_stream.try_clone()?;
    let buf_tx = BufWriter::new(tcp_tx);
    let buf_rx = BufReader::new(tcp_stream);
    Ok((buf_tx, buf_rx))
}

fn recv(buf_rx: BufReader<std::net::TcpStream>) -> thread::JoinHandle<()> {
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
    recv_task
}

fn write(buf_tx: BufWriter<std::net::TcpStream>, req_content: String) -> thread::JoinHandle<()> {
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
    send_task
}
