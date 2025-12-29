use anyhow::Result;
use smol::net::{TcpListener as SmolTcpListener, TcpStream as SmolTcpStream};

use crate::{
    cmd::Args,
    serve::{GracefulShutdown, StaticServeService},
};

mod app;
mod cmd;
mod serve;
mod utils;

pub async fn run() -> Result<()> {
    let ctrlc2 = app::ctrlc::init()?;
    let args = app::cmd::init()?;
    serve(args, ctrlc2).await
}

async fn serve(args: Args, ctrlc2: ctrlc2::AsyncCtrlC) -> Result<()> {
    let serve_path = args.serve.expect("should have a valid path for serving");
    let bind_addr = args.bind.expect("should have a valid bind addr");
    let tcp_listener = SmolTcpListener::bind(*bind_addr).await?;
    let local_addr = tcp_listener.local_addr()?;
    log::info!("Server listen on: http://{}", local_addr);

    let shutdown = GracefulShutdown::new();
    let shutdown_for_signal = shutdown.clone();
    smol::spawn(async move {
        let _ = ctrlc2.await;
        log::info!("Shutdown requested (Ctrl+C). Waiting for in-flight requests...");
        shutdown_for_signal.initiate().await;
        // Wake the accept() loop so it can observe the shutdown flag.
        let _ = SmolTcpStream::connect(local_addr).await;
    })
    .detach();

    StaticServeService::new(&serve_path)
        .serve(tcp_listener, shutdown)
        .await?;
    log::info!("Shutdown complete.");
    Ok(())
}
