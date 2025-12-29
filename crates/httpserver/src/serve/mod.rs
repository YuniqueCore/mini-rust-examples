use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
    time::Instant,
};

use anyhow::Result;
use futures::FutureExt;
use smol::future;
use smol::net::TcpListener as SmolTcpListener;

mod common;
mod fs;
mod http;
mod request;
mod response;
mod runtime;
mod ui;
mod util;

pub(crate) use fs::static_files;
pub(crate) use http::{auth, connection};
pub(crate) use runtime::shutdown;
pub(crate) use ui::render;
pub use util::url;

pub use fs::types;

use auth::BasicAuth;
pub use common::*;
pub use response::*;
pub(crate) use shutdown::GracefulShutdown;
use types::TypeMappings;

#[macro_export]
macro_rules! impl_deref_mut {
    (
        $struct:ident ( $target:ident )
    ) => {
        impl Deref for $struct {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $struct {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

#[derive(Debug)]
pub struct StaticServeService {
    /// Base directory; static files are served from this directory.
    serve_path: PathBuf,
    started_at: Instant,
    request_count: Arc<AtomicU64>,
    types: TypeMappings,
    auth: Option<BasicAuth>,
}

impl StaticServeService {
    pub fn new(serve_path: &Path, types: TypeMappings, auth: Option<BasicAuth>) -> Self {
        Self {
            serve_path: serve_path.to_path_buf(),
            started_at: Instant::now(),
            request_count: Arc::new(AtomicU64::new(0)),
            types,
            auth,
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
            let auth = self.auth.clone();
            let started_at = self.started_at;
            let request_count = self.request_count.clone();
            let shutdown = shutdown.clone();
            smol::spawn(async move {
                let run = async move {
                    let _inflight = shutdown.inflight_guard();
                    let ctx = connection::ConnectionContext {
                        serve_path,
                        types,
                        auth,
                        started_at,
                        request_count,
                        shutdown: shutdown.clone(),
                    };
                    connection::handle(stream, peer, ctx).await
                };

                match std::panic::AssertUnwindSafe(run).catch_unwind().await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        log::debug!("Connection {peer} closed with error: {e:#}");
                    }
                    Err(_) => {
                        log::error!("Connection {peer} panicked.");
                    }
                }
            })
            .detach();
        }

        shutdown.wait_inflight_zero().await;
        Ok(())
    }
}
