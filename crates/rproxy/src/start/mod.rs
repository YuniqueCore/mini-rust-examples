use std::net::Shutdown;
use std::net::SocketAddr;

use anyhow::Result;
use smol::{
    future::{self},
    net::{TcpListener, TcpStream},
};

use crate::init::shutdown::GracefulShutdown;

pub async fn handle_local_target(
    bind: SocketAddr,
    reverse_addr: SocketAddr,
    shutdown: &GracefulShutdown,
) -> Result<()> {
    let client_tcp_listener = TcpListener::bind(bind).await?;

    loop {
        let Some((local_stream, peer)) = accept_or_shutdown(&client_tcp_listener, shutdown).await?
        else {
            break;
        };

        log::info!("accept connection from {peer}");

        let shutdown_signal = shutdown.clone();
        // NOTE: 反向代理/端口转发里，上游 connect 可能会比较慢或失败。
        // 放到单独任务里，避免阻塞 accept 循环；并且单个连接失败不影响整个 listener。
        smol::spawn(async move {
            let _guard = shutdown_signal.inflight_guard();
            match TcpStream::connect(reverse_addr).await {
                Ok(reverse_stream) => {
                    log::info!("peer={peer} connected reverse {reverse_addr}");
                    if let Err(err) = tunnel(local_stream, reverse_stream).await {
                        log::warn!("peer={peer} tunnel error: {err}");
                    }
                }
                Err(err) => {
                    log::warn!("peer={peer} connect reverse {reverse_addr} failed: {err}");
                }
            }
        })
        .detach();
    }

    let _ = shutdown.wait_inflight_zero().await;

    Ok(())
}

async fn accept_or_shutdown(
    client_tcp_listener: &TcpListener,
    shutdown: &GracefulShutdown,
) -> std::io::Result<Option<(TcpStream, SocketAddr)>> {
    let accept_handle = async { client_tcp_listener.accept().await.map(Some) };

    let shutdown_handle = async {
        shutdown.wait_shutting_down().await;
        Ok(None)
    };

    future::or(accept_handle, shutdown_handle).await
}

async fn tunnel(local_stream: TcpStream, reverse_stream: TcpStream) -> std::io::Result<()> {
    let local_tx = local_stream.clone();
    let local_rx = local_stream.clone();
    let reverse_tx = reverse_stream.clone();
    let reverse_rx = reverse_stream.clone();

    // Q: 明明不需要 mut, 经过测试, mut 和 不使用 mut 效果都是一致的, 那么为什么我看的那个教程使用了 mut
    // ANSWER:
    // - 先看你当前用的 `smol::io::copy`（实际来自 `futures-lite`）：它的签名是 `copy(reader: R, writer: W)`，
    //   也就是 **按值接收** reader/writer，并在内部把它们 pin 起来反复 poll 读写，因此你在外面不需要写 `&mut xxx`。
    //   参考：`/Users/unic/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/futures-lite-2.6.1/src/io.rs` 的 `pub async fn copy<R, W>(reader: R, writer: W)`.
    // - 很多教程（尤其是 tokio 或 std 的）用的是 `tokio::io::copy(&mut R, &mut W)` / `std::io::copy(&mut R, &mut W)`：
    //   这种 **按可变借用** 的 API 要求你能拿到 `&mut`，所以变量通常要 `let mut r = ...;` 才能借用成 `&mut r`。
    // - 结论：要不要 `mut` 取决于你调用的 API 形态：
    //   - futures-lite/smol: `io::copy(reader, writer)`（按值，外部不一定要 `mut`）
    //   - tokio/std: `io::copy(&mut reader, &mut writer)`（按引用，需要 `mut` 才能借出 `&mut`）
    // - 顺带一提：这里虽然“看起来是双向 copy”，但如果只 `race` 等一个方向结束，可能会在“半关闭”场景截断数据：
    //   例如 client 发完请求后 `shutdown(SHUT_WR)`，client->server 方向先 EOF；如果你立刻结束并取消 server->client，
    //   server 的响应可能还没转发完。下面用 shutdown(write) + 等待另一方向结束来更符合 TCP 半关闭语义。
    // let local_len =
    //     smol::spawn(async move { smol::io::copy(&mut local_rx, &mut reverse_tx).await });
    // let reverse_len =
    //     smol::spawn(async move { smol::io::copy(&mut reverse_rx, &mut local_tx).await });

    let mut local_to_reverse =
        smol::spawn(async move { smol::io::copy(local_rx, reverse_tx).await });
    let mut reverse_to_local =
        smol::spawn(async move { smol::io::copy(reverse_rx, local_tx).await });

    enum Finished {
        LocalToReverse,
        ReverseToLocal,
    }

    let (finished, first) = future::race(
        async { (Finished::LocalToReverse, (&mut local_to_reverse).await) },
        async { (Finished::ReverseToLocal, (&mut reverse_to_local).await) },
    )
    .await;

    match finished {
        Finished::LocalToReverse => {
            let _ = reverse_stream.shutdown(Shutdown::Write);
            let _ = reverse_to_local.await;
        }
        Finished::ReverseToLocal => {
            let _ = local_stream.shutdown(Shutdown::Write);
            let _ = local_to_reverse.await;
        }
    }

    first.map(|_| ())
}
