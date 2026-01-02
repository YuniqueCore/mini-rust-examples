use anyhow::Result;

mod cmd;
mod common;
mod init;
mod start;

pub async fn run() -> Result<()> {
    let shutdown = init::shutdown::init()?;

    let args = init::cmd::init()?;

    let bind_addr = *args.bind.expect("should has a valid bind address");
    start::handle_local_target(bind_addr, &shutdown).await?;
    log::info!("Shutdown complete.");
    Ok(())
}
