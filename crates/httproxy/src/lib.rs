use std::{str::FromStr, thread, time::Duration};

use anyhow::Result;

use crate::{cmd::LogLevel};

mod cmd;
mod common;
mod init;
mod start;

pub async fn run() -> Result<()> {
    let shutdown = init::shutdown::init()?;

    let args = init::cmd::init()?;

    let shutdown_clone = shutdown.clone();
    // smol::spawn(async move {
    //     let guard = shutdown_clone.inflight_guard();
    //     thread::sleep(Duration::from_secs(5));
    //     drop(guard);
    // }).detach();

    log::info!("Shutdown complete.");
    Ok(())
}


