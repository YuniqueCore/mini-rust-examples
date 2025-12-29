use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
    time::Instant,
};

use anyhow::Result;
use smol::future;
use smol::net::TcpListener as SmolTcpListener;

mod common;
mod connection;
mod render;
mod request;
mod response;
mod shutdown;
mod static_files;
mod url;

pub mod types;

pub use common::*;
pub use response::*;
pub(crate) use shutdown::GracefulShutdown;
use types::TypeMappings;

#[derive(Debug)]
pub struct StaticServeService {
    /// Base directory; static files are served from this directory.
    serve_path: PathBuf,
    started_at: Instant,
    request_count: Arc<AtomicU64>,
    types: TypeMappings,
}

impl StaticServeService {
    pub fn new(serve_path: &Path, types: TypeMappings) -> Self {
        Self {
            serve_path: serve_path.to_path_buf(),
            started_at: Instant::now(),
            request_count: Arc::new(AtomicU64::new(0)),
            types,
        }
    }

    pub async fn serve(
        &self,
        tcp_listener: SmolTcpListener,
        shutdown: GracefulShutdown,
    ) -> Result<()> {
        loop {
            let accept_fut = async { tcp_listener.accept().await.map(Some) };
            let shutdown_fut = async {
                shutdown.wait_shutting_down().await;
                Ok(None)
            };
            let accepted = future::or(accept_fut, shutdown_fut).await?;
            let Some((stream, peer)) = accepted else {
                break;
            };
            log::debug!("Accepted connection from {peer}");

            if shutdown.is_shutting_down() {
                drop(stream);
                break;
            }

            let serve_path = self.serve_path.clone();
            let types = self.types.clone();
            let started_at = self.started_at;
            let request_count = self.request_count.clone();
            let shutdown = shutdown.clone();
            smol::spawn(async move {
                let _inflight = shutdown.inflight_guard();
                if let Err(e) = connection::handle(
                    stream,
                    peer,
                    &serve_path,
                    &types,
                    started_at,
                    request_count,
                    shutdown.clone(),
                )
                .await
                {
                    log::debug!("Connection {peer} closed with error: {e:#}");
                }
            })
            .detach();
        }

        shutdown.wait_inflight_zero().await;
        Ok(())
    }
}
