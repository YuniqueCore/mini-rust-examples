use anyhow::Result;
use smol::{future, net::TcpListener as SmolTcpListener};

use crate::{cmd::{Args}, serve::StaticServeService
};

mod cmd;
mod app;
mod serve;
mod utils;

pub async fn run() -> Result<()> {
    let ctrlc2 = app::ctrlc::init()?;
    let args = app::cmd::init()?;
    let termination = smol::spawn(async {
        let _ = ctrlc2.await;
        log::debug!("Ctrl+C received, starting shutdown...");
        Ok(())
    });
    let service = serve(args);

    future::race(service, termination).await?;
    Ok(())
}

async fn serve(args: Args) -> Result<()> {
    let serve_path = args.serve.expect("should have a valid path for serving");
    let bind_addr = args.bind.expect("should have a valid bind addr");
    let tcp_listener = SmolTcpListener::bind(*bind_addr).await?;
    StaticServeService::new(&serve_path)
        .serve(tcp_listener)
        .await
}


