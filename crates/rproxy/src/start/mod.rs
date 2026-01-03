use anyhow::Result;
use httparse::Header;
use smol::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use std::net::SocketAddr;

use crate::init::shutdown::GracefulShutdown;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug)]
struct ClientRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct UpstreamResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub async fn handle_local_target(bind_addr: SocketAddr, shutdown: &GracefulShutdown) -> Result<()> {
    let tcp_listener = TcpListener::bind(bind_addr).await?;
    log::info!("httproxy listening on {bind_addr}");

    loop {
        let Some((stream, peer)) = accept_or_shutdown(&tcp_listener, shutdown).await? else {
            break;
        };

        let shutdown = shutdown.clone();
        smol::spawn(async move {
            let _guard = shutdown.inflight_guard();
            if let Err(err) = handle_client(stream, peer).await {
                log::warn!("peer={peer} error: {err}");
            }
        })
        .detach();
    }

    shutdown.wait_inflight_zero().await;
    Ok(())
}

async fn accept_or_shutdown(
    listener: &TcpListener,
    shutdown: &GracefulShutdown,
) -> std::io::Result<Option<(TcpStream, SocketAddr)>> {
    let accept_fut = async { listener.accept().await.map(Some) };
    let shutdown_fut = async {
        shutdown.wait_shutting_down().await;
        Ok(None)
    };
    future::or(accept_fut, shutdown_fut).await
}

async fn handle_client(mut client_stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let req = match read_client_request(&mut client_stream, peer).await {
        Ok(req) => req,
        Err(err) => {
            write_plain_error(
                &mut client_stream,
                400,
                "Bad Request",
                format!("Bad Request: {err}\n"),
            )
            .await?;
            return Ok(());
        }
    };

    if req.method.eq_ignore_ascii_case("CONNECT") {
        // QUESTION:  这里为什么将 req.path 看作 authority? 为什么叫 authority
        // 这里的 path 是 ip 还是 domain? 还是别的?
        // 以及如果不是 domain, 因为有 CONNECT, 所以就是 https, 而 https port == 443, 所以才添加 :443 嘛?
        // ANSWER:
        // - 在 HTTP/1.1 里，CONNECT 的 request-target 不是普通的 “/path”（origin-form），而是 authority-form：`host:port`。
        //   httparse 统一把 request-target 放在 `req.path` 字段里，但对 CONNECT 来说它的语义就是“要建立 TCP 隧道的目标地址”。
        // - 这里的 `path` 既可能是域名也可能是 IP：
        //   - `CONNECT google.com:443 HTTP/1.1`
        //   - `CONNECT 142.251.34.206:443 HTTP/1.1`
        //   - IPv6 一般写成 `CONNECT [2001:db8::1]:443 HTTP/1.1`
        // - 按常见客户端行为，CONNECT 会显式带端口；这里为了学习/容错，没带端口就默认补 `:443`。
        //   但要注意：HTTPS 并不“只等于 443”（也可能是 8443 等），生产级代理通常不会盲猜端口，而是要求客户端提供。
        let mut authority = req.path;
        if !authority.contains(':') {
            authority.push_str(":443");
        }

        let mut remote_stream = match TcpStream::connect(authority.as_str()).await {
            Ok(s) => s,
            Err(err) => {
                write_plain_error(
                    &mut client_stream,
                    502,
                    "Bad Gateway",
                    format!("CONNECT failed: {err}\n"),
                )
                .await?;
                return Ok(());
            }
        };

        client_stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        client_stream.flush().await?;

        // QUESTION: 不需要一次性的将 request_line, headers, body 都发送出去嘛?
        // 虽然我知道对面也是使用 tcp stream 进行流读取, 但是分段,和完整发送都是没有问题的嘛?
        // 具体的话推荐那种? 为什么?
        // ANSWER:
        // - TCP 是“字节流”协议，没有消息边界：你在发送端 `write()` 一次还是多次，
        //   接收端的 `read()` 看到的都是同一条连续字节流，可能被合并也可能被拆分（由内核缓冲、MSS、Nagle 等决定）。
        //   所以从协议正确性角度，“分段发送”和“一次性发送”都没问题。
        // - 但这里还有一个更关键的点：对 CONNECT 来说，`\r\n\r\n` 之后就不再是 HTTP body，而是“隧道里的原始字节”（通常是 TLS 握手）。
        //   我们在读 CONNECT 头时，可能一次 `read()` 就把一部分 TLS ClientHello 也读进来了，于是它落在 `req.body`（更准确是 pre_body）里。
        //   这些字节如果不先转发给 remote，remote 端会“少收到握手开头”，TLS 就会失败。
        // - 一个真实的数据流示例（同一个 TCP 包里同时出现 CONNECT 头 + TLS ClientHello 很常见）：
        //   1) client -> proxy: `CONNECT google.com:443 HTTP/1.1\r\nHost: google.com:443\r\n\r\n` + (紧跟着) TLS ClientHello...
        //   2) proxy  -> client: `HTTP/1.1 200 Connection Established\r\n\r\n`
        //   3) proxy  -> remote: 先把已经读到的 ClientHello 字节写出去
        //   4) 然后进入 `tunnel()`，后续字节由双向 `io::copy` 持续转发
        // - 性能建议：更少的 `write()` syscall 通常更快；但学习项目里保持简单（`write_all` + `tunnel`）更重要。
        if !req.body.is_empty() {
            remote_stream.write_all(&req.body).await?;
            remote_stream.flush().await?;
        }

        log::info!("peer={peer} CONNECT {authority}");
        return tunnel(client_stream, remote_stream).await;
    }

    log::info!("peer={peer} {} {}", req.method, req.path);

    match forward_via_ureq(req).await {
        Ok(resp) => write_response(&mut client_stream, &resp).await?,
        Err(err) => {
            log::debug!("peer={peer} upstream error: {err}");
            write_plain_error(
                &mut client_stream,
                502,
                "Bad Gateway",
                format!("Bad Gateway: {err}\n"),
            )
            .await?;
        }
    }

    Ok(())
}

async fn read_client_request(stream: &mut TcpStream, peer: SocketAddr) -> Result<ClientRequest> {
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];

    let header_end = loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("peer closed connection: {peer}"));
        }
        // QUESTION: 这里的 extend_from_slice 其实不太好, 因为其源代码中存在这样的代码, 会导致性能问题
        // self.set_len(len + 1); 当容量不够时, 每次只增加 1...
        // ANSWER:
        // - 这里是 `Vec<u8>::extend_from_slice(&tmp[..n])`。对 `u8` 这种 Copy 类型，标准库会走专门的优化路径：
        //   先 `reserve(n)` 确保容量足够，再用一次 `memcpy`（`copy_nonoverlapping`）把 n 个字节拷进 Vec，并把 len 直接 +n。
        //   不会出现“每个字节 set_len(len+1)”那种逐个 push 的扩容/拷贝开销。
        //   （可参考 Rust 源码：`alloc/src/vec/spec_extend.rs` 的 slice 特化，以及 `alloc/src/vec/mod.rs` 的 `append_elements`）
        // - 你提到的 `set_len(len + 1)` 更多是通用 `extend_desugared`/逐元素 `push` 的路径；它也不等于“容量每次只 +1”，
        //   因为真正的扩容由 `reserve`/增长策略决定（通常是几何增长，摊还 O(1)）。
        // - 对网络读包来说，真正的大头通常是系统调用/拷贝次数；学习项目里用 `extend_from_slice` 是合理的取舍。
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_HEADER_BYTES {
            return Err(anyhow::anyhow!("request headers too large"));
        }
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let head = &buf[..header_end];

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(head)? {
        httparse::Status::Complete(_) => {}
        httparse::Status::Partial => return Err(anyhow::anyhow!("incomplete request headers")),
    }

    let method = req
        .method
        .ok_or_else(|| anyhow::anyhow!("missing method"))?;
    let path = req.path.ok_or_else(|| anyhow::anyhow!("missing path"))?;
    let _version = req
        .version
        .ok_or_else(|| anyhow::anyhow!("missing version"))?;

    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).to_string(),
            )
        })
        .collect();

    let pre_body = buf[header_end..].to_vec();
    // QUESTION: HTTPS 一定会有 CONNECT !? 为什么, HTTPS 和 HTTP 在实际的数据交互方面有什么差别?!
    // 这些差异怎么影响了代码的处理逻辑
    // ANSWER:
    // - “HTTPS 一定会有 CONNECT”只在“客户端通过 *HTTP 前向代理* 访问 `https://...` 目标”这个场景里成立。
    //   如果客户端直连 HTTPS 服务器，那就不会出现 CONNECT；如果是 MITM/透明代理，也可能看不到标准 CONNECT（实现方式不同）。
    // - HTTP(明文) 的数据流：client 直接把 `GET /path ...` + headers + body 发给代理/服务器，代理可以解析并重放请求。
    // - HTTPS 的 HTTP 报文被 TLS 加密：在不做 MITM 的情况下，代理无法看见里面的 method/path/header/body，
    //   所以常见做法是用 CONNECT 让代理只负责“建立到目标的 TCP 连接”，随后代理只做字节转发（tunnel）。
    // - 因此代码上：遇到 CONNECT 我们只需要解析出目标 `authority` 并进入 `tunnel()`；非 CONNECT 才去按 HTTP 语义读取 Content-Length 等。
    if method.eq_ignore_ascii_case("CONNECT") {
        return Ok(ClientRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body: pre_body,
        });
    }

    if header_has_value(req.headers, "transfer-encoding", "chunked") {
        return Err(anyhow::anyhow!("chunked request body not supported"));
    }

    let content_length = parse_content_length(req.headers)?;
    let body = if let Some(len) = content_length {
        if len > MAX_BODY_BYTES {
            return Err(anyhow::anyhow!("request body too large: {len} bytes"));
        }

        let mut body = pre_body;
        while body.len() < len {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(anyhow::anyhow!("peer closed connection while reading body"));
            }
            body.extend_from_slice(&tmp[..n]);
            if body.len() > len {
                body.truncate(len);
                break;
            }
        }
        body.truncate(len);
        body
    } else {
        if !pre_body.is_empty() {
            log::debug!("peer={peer} extra bytes after headers are ignored (no Content-Length)");
        }
        Vec::new()
    };

    Ok(ClientRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    })
}

fn parse_content_length(headers: &[Header<'_>]) -> Result<Option<usize>> {
    let Some(h) = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
    else {
        return Ok(None);
    };
    let s = std::str::from_utf8(h.value)?.trim();
    if s.is_empty() {
        return Ok(None);
    }
    Ok(Some(s.parse()?))
}

fn header_has_value(headers: &[Header<'_>], name: &str, expected: &str) -> bool {
    headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case(name)
            && std::str::from_utf8(h.value)
                .ok()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case(expected))
    })
}

async fn forward_via_ureq(req: ClientRequest) -> Result<UpstreamResponse> {
    let url = build_target_url(&req.path, &req.headers)?;

    // QUESTION: 为什么使用的是 unblock? 而不是 spawn 然后 await?
    // ANSWER:
    // - `ureq` 是同步/阻塞式 HTTP 客户端（内部会做阻塞 DNS/TCP/TLS/读写）。
    // - `smol::spawn(async { ... })` 只是把 future 放到 async 执行器里跑：如果你在里面直接调用阻塞 IO，
    //   会把执行器线程卡住，导致同一执行器上的其他连接无法及时被 poll（accept/read/write 变慢，甚至“全局卡顿”）。
    // - `smol::unblock` 会把这段阻塞工作丢到专门的 blocking 线程池里执行，然后 async 地等待结果返回，
    //   这样不会阻塞 smol 的 reactor/executor。
    // - 一个直观例子：如果同时有 100 个客户端请求，而每个上游请求都可能卡 500ms，
    //   用 `spawn + 阻塞调用` 会让 100 个任务把 executor 线程堵死；用 `unblock` 则只会占用 blocking 线程池。
    smol::unblock(move || {
        let mut builder = ureq::http::Request::builder()
            .method(req.method.as_str())
            .uri(url.as_str());

        for (name, value) in req.headers {
            if should_skip_request_header(&name) {
                continue;
            }
            builder = builder.header(name.as_str(), value.as_str());
        }

        builder = builder.header("accept-encoding", "identity");
        builder = builder.header("connection", "close");

        builder = builder.header("content-length", req.body.len().to_string());

        let request = builder.body(req.body)?;
        let agent: ureq::Agent = ureq::config::Config::builder()
            .proxy(None)
            .max_redirects(0)
            .build()
            .into();
        let resp = agent.run(request)?;

        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    // QUESTION: 这里直接变成了 string, 是不是不太好? 因为可能会有编码问题?
                    // 而是保持原始的 [u8] 更好?
                    // ANSWER:
                    // - 更“严格”的 HTTP 实现确实应该把 header value 当作字节序列处理，而不是强行当 UTF-8。
                    // - 但在 HTTP/1.1 里，大多数 header value 实际上都是可打印 ASCII（例如 Location、Content-Type、Server 等），
                    //   `from_utf8_lossy` 通常不会丢信息；学习项目这样做可以简化 `build_response_bytes()` 的拼接逻辑。
                    // - 可能出问题的例子：如果某个上游返回了非 UTF-8 的 header value（极少见/不规范），
                    //   `from_utf8_lossy` 会用 U+FFFD 替换非法字节，导致我们转发出去的值被“改变”。
                    // - 如果要更严谨：可以把结构改为 `Vec<(String, Vec<u8>)>`，然后在写回客户端时直接写原始字节（避免编码假设）。
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect();

        let mut body = resp.into_body();
        let body = body.read_to_vec()?;

        Ok(UpstreamResponse {
            status,
            headers,
            body,
        })
    })
    .await
}

// QUESTION: 这里这样处理, 是为了应对 redirect 这样的情况嘛?
// ANSWER:
// - 这里主要是在兼容不同的 request-target 形式（而不是专门为 redirect）。
// - 作为“显式 HTTP 代理”时，客户端常会发 absolute-form：
//   `GET http://example.com/path HTTP/1.1`
//   这种情况下 `path` 本身就是完整 URL，我们直接用它即可。
// - 但有些客户端/场景会发 origin-form（更像直连服务器的请求）：
//   `GET /path HTTP/1.1` + `Host: example.com`
//   这时我们需要用 Host 把 URL 补全，否则上游 HTTP 客户端（ureq）不知道该连哪个主机。
// - CONNECT 则是 authority-form（`host:port`），它在上面已被单独处理，不走这里。
fn build_target_url(path: &str, headers: &[(String, String)]) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }

    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing Host header"))?;

    let path = if path.starts_with('/') || path == "*" {
        path.to_string()
    } else {
        format!("/{path}")
    };

    Ok(format!("http://{host}{path}"))
}

// QUESTION: 为什么需要跳过 request 的 header, 场景以及原因
// ANSWER:
// - 代理转发时要避免把“逐跳(hop-by-hop)”的 header 转发给上游，因为这些 header 只对当前这条 TCP 连接有效，
//   转发过去会产生错误语义（例如让上游误以为要 keep-alive、或错误处理 Transfer-Encoding）。
// - 我们还会跳过一些由 ureq/本代理自行生成的 header，避免产生“重复/冲突”的值：
//   - `Host`：应由最终 URI 决定；如果仍用客户端的 Host，重定向/不同域名时可能导致 Host 与连接目标不一致。
//   - `Content-Length`：我们根据实际 body 长度重新设置（本实现只支持 Content-Length）。
//   - `Accept-Encoding`：我们强制 `identity`，避免压缩/解压带来的 body 长度与 header 不一致问题。
// - 一个实际例子：curl 走代理会带 `Proxy-Connection: Keep-Alive`，
//   这个 header 只对“curl <-> 代理”这一跳有意义，转发给 “代理 <-> google” 反而会让对端行为不可预期。
fn should_skip_request_header(name: &str) -> bool {
    is_hop_by_hop_header(name)
        || name.eq_ignore_ascii_case("accept-encoding")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("host")
}

// QUESTION: 为什么需要跳过 response 的 header, 场景以及原因
// ANSWER:
// - 原因同样是 hop-by-hop header 不应该被代理跨连接转发。
// - 另外本实现会“重建”响应：把上游 body 全部读到内存后再写回客户端，并且统一输出 `Content-Length` + `Connection: close`。
//   因此我们必须跳过一些和 body 表示方式强相关的 header，否则会造成客户端解析错误：
//   - `Transfer-Encoding`: 如果上游是 chunked，而我们写回的不是 chunked，客户端会按 chunked 解析导致失败。
//   - `Content-Encoding`: 如果上游返回 gzip，但我们写回的是已解压/或非 gzip 内容，客户端会错误解码。
//   - `Content-Length`: 我们会按实际写回的 body 长度重新生成，避免长度不匹配导致客户端卡住或截断。
// - 更“透明”的代理会选择流式转发并尽量保留原始 header（包括 chunked），但那会让实现明显复杂很多。
fn should_skip_response_header(name: &str) -> bool {
    is_hop_by_hop_header(name)
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("content-encoding")
}

// QUESTION: 这里是在干嘛? 为什么需要做这个
// ANSWER:
// - 这是在判定某个 header 是否属于 hop-by-hop（逐跳）header。
// - hop-by-hop header 只对“当前这一跳”生效：也就是一条 TCP 连接上的双方约定，不应该被代理转发到下一跳。
// - 典型例子：
//   - `Connection: keep-alive` / `Proxy-Connection: keep-alive`：只影响当前连接是否复用
//   - `Upgrade` / `TE` / `Trailer` / `Transfer-Encoding`：和当前连接上的编码/升级相关
// - 我们在 `should_skip_request_header` / `should_skip_response_header` 里复用它，避免重复写一堆 `eq_ignore_ascii_case`。
fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
    )
}

async fn write_response(stream: &mut TcpStream, resp: &UpstreamResponse) -> Result<()> {
    let bytes = build_response_bytes(resp);
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

fn build_response_bytes(resp: &UpstreamResponse) -> Vec<u8> {
    let status = ureq::http::StatusCode::from_u16(resp.status)
        .unwrap_or(ureq::http::StatusCode::INTERNAL_SERVER_ERROR);
    let reason = status.canonical_reason().unwrap_or("");

    let mut out: Vec<u8> = Vec::with_capacity(1024 + resp.body.len());
    out.extend_from_slice(format!("HTTP/1.1 {} {reason}\r\n", status.as_u16()).as_bytes());

    for (name, value) in &resp.headers {
        if should_skip_response_header(name) {
            continue;
        }
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }

    out.extend_from_slice(format!("Content-Length: {}\r\n", resp.body.len()).as_bytes());
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(&resp.body);
    out
}

async fn write_plain_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: String,
) -> Result<()> {
    let bytes = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(bytes.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

// QUESTION: 为什么 TcpStream 可以 clone 之后就作为 tx, rx 使用? 而不会冲突!?
// ANSWER:
// - `smol::net::TcpStream` 的 `clone()` 本质上是“复制/共享同一个底层 socket 句柄”，不是复制一条新连接。
//   两个 clone 指向同一个 TCP 连接的读写缓冲区。
// - 对 socket 来说：
//   - 多个 writer 同时写：字节会交错，容易把协议写乱（所以我们避免并发写同一方向）
//   - 多个 reader 同时读：会“抢”同一个接收缓冲，导致数据被不确定地分配给不同 reader（也要避免）
// - 这里的用法是“模拟 split”：
//   - `client_read` 只负责读 client -> remote
//   - `client_write` 只负责写 remote -> client
//   - `remote_read` 只负责读 remote -> client
//   - `remote_write` 只负责写 client -> remote
//   每个方向只有一个 reader 和一个 writer，所以不会产生读/写竞争。
// - 数据流示意：
//   client_read  --(io::copy)-->  remote_write
//   remote_read  --(io::copy)-->  client_write
// - 注意：`io::copy` 会一直跑到 EOF（read 返回 0）才结束；更完整的实现会处理“半关闭/超时/任一方向结束就关闭另一方向”等边界情况。
async fn tunnel(client: TcpStream, remote: TcpStream) -> Result<()> {
    let mut client_read = client.clone();
    let mut client_write = client;
    let mut remote_read = remote.clone();
    let mut remote_write = remote;

    let c2r = smol::spawn(async move { smol::io::copy(&mut client_read, &mut remote_write).await });
    let r2c = smol::spawn(async move { smol::io::copy(&mut remote_read, &mut client_write).await });

    let _ = c2r.await?;
    let _ = r2c.await?;
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_target_url_absolute() -> Result<()> {
        let headers = vec![("Host".to_string(), "example.com".to_string())];
        let url = build_target_url("http://example.com/a", &headers)?;
        assert_eq!(url, "http://example.com/a");
        Ok(())
    }

    #[test]
    fn test_build_target_url_origin_form() -> Result<()> {
        let headers = vec![("Host".to_string(), "example.com:8080".to_string())];
        let url = build_target_url("/hello", &headers)?;
        assert_eq!(url, "http://example.com:8080/hello");
        Ok(())
    }

    #[test]
    fn test_hop_by_hop_headers() {
        assert!(is_hop_by_hop_header("Connection"));
        assert!(is_hop_by_hop_header("proxy-connection"));
        assert!(is_hop_by_hop_header("TRANSFER-ENCODING"));
        assert!(!is_hop_by_hop_header("content-type"));
    }
}
