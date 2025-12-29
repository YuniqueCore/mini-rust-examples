use std::{str::FromStr, thread, time::Duration};

use anyhow::Result;

use crate::{cmd::LogLevel};

mod cmd;
mod common;
mod init;

pub async fn run() -> Result<()> {
    let shutdown = init::shutdown::init()?;

    let args = init::cmd::init()?;
    let _ = init::logger::init(
        &args.log_level.unwrap_or(LogLevel::from_str("info")?),
        args.colored.unwrap_or(false),
    )?;


    let shutdown_clone = shutdown.clone();
    smol::spawn(async move {
        let guard = shutdown_clone.inflight_guard();
        thread::sleep(Duration::from_secs(10));
        drop(guard);
    }).await;

    log::info!("Shutdown complete.");
    Ok(())
}


