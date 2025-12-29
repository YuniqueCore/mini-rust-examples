use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use anyhow::Result;
use mea::{condvar::Condvar, mutex::Mutex};

mod ctrlc;

pub fn init() ->Result<GracefulShutdown> {
    let ctrlc = ctrlc::init()?;
    let shutdown = GracefulShutdown::new();
    termination(ctrlc, shutdown.clone());
    Ok(shutdown)
}

fn termination(ctrlc:ctrlc2::AsyncCtrlC, shutdown_for_signal:GracefulShutdown) {
    smol::spawn(async move {
        let _ = ctrlc.await;
        log::info!("Shutdown requested (Ctrl+C). Waiting for in-flight requests...");
        shutdown_for_signal.initiate();
    })
    .detach();
}


#[derive(Clone, Debug)]
pub(crate) struct GracefulShutdown {
    inner: Arc<GracefulShutdownInner>,
}

#[derive(Debug)]
struct GracefulShutdownInner {
    shutting_down: AtomicBool,
    inflight: AtomicU64,
    gate: Mutex<()>,
    cv: Condvar,
}

#[derive(Debug)]
pub(crate) struct InflightGuard {
    inner: Arc<GracefulShutdownInner>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        if self.inner.inflight.fetch_sub(1, Ordering::AcqRel) == 1 {
            // last in-flight request finished
            self.inner.cv.notify_all();
        }
    }
}

impl GracefulShutdown {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GracefulShutdownInner {
                shutting_down: AtomicBool::new(false),
                inflight: AtomicU64::new(0),
                gate: Mutex::new(()),
                cv: Condvar::new(),
            }),
        }
    }

    pub fn initiate(&self) {
        if self.inner.shutting_down.swap(true, Ordering::Release) {
            return;
        }
        self.inner.cv.notify_all();
    }

    pub fn is_shutting_down(&self) -> bool {
        self.inner.shutting_down.load(Ordering::Acquire)
    }

    pub async fn wait_shutting_down(&self) {
        if self.is_shutting_down() {
            return;
        }
        let mut guard = self.inner.gate.lock().await;
        while !self.is_shutting_down() {
            guard = self.inner.cv.wait(guard).await;
        }
    }

    pub fn inflight_guard(&self) -> InflightGuard {
        self.inner.inflight.fetch_add(1, Ordering::Relaxed);
        InflightGuard {
            inner: self.inner.clone(),
        }
    }

    pub async fn wait_inflight_zero(&self) {
        if self.inner.inflight.load(Ordering::Acquire) == 0 {
            return;
        }
        let mut guard = self.inner.gate.lock().await;
        while self.inner.inflight.load(Ordering::Acquire) != 0 {
            guard = self.inner.cv.wait(guard).await;
        }
    }
}
