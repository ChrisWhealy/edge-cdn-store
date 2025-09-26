mod display_disk_cache;
mod routes;

use crate::{disk_cache::DiskCache, inspector::routes::build_inspector_routes};

use async_trait::async_trait;
use pingora_core::{server::ShutdownWatch, services::background::BackgroundService};
use std::{sync::Arc, thread};
use tokio::sync::{oneshot, Mutex};
use warp::reply::Response as WarpResponse;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
const VERSION_PATH: &'static str = "version";
const HEALTH_PATH: &'static str = "health";
const STATS_PATH: &'static str = "stats";
const METRICS_PATH: &'static str = "metrics";
const CACHE_CONTENTS_PATH: &'static str = "cache";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct InspectorHandle {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl InspectorHandle {
    pub fn new(tx: oneshot::Sender<()>, thread: thread::JoinHandle<()>) -> Self {
        Self {
            shutdown_tx: Mutex::new(Some(tx)),
            thread: Mutex::new(Some(thread)),
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct StopInspectorOnShutdown {
    pub inspector: Arc<InspectorHandle>,
}

#[async_trait]
impl BackgroundService for StopInspectorOnShutdown {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        // Wait for graceful shutdown
        // This is only fired for SIGTERM or SIGQUIT, NOT SIGINT
        let _ = shutdown.changed().await;

        // Signal warp to stop
        if let Some(tx) = self.inspector.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
            tracing::info!("inspector: sent shutdown");
        } else {
            tracing::warn!("inspector: shutdown signal already sent or not initialized");
        }

        // Joining a std::thread is blocking; do it off the reactor:
        if let Some(th) = self.inspector.thread.lock().await.take() {
            tokio::task::spawn_blocking(move || {
                let _ = th.join();
                tracing::info!("inspector: thread joined");
            });
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn start_disk_cache_inspector(addr: std::net::SocketAddr, cache: Arc<&'static DiskCache>) -> Arc<InspectorHandle> {
    let (tx, rx) = oneshot::channel::<()>();
    let routes = build_inspector_routes(cache);

    let th = std::thread::Builder::new()
        .name("disk cache inspector".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("inspector: build tokio rt");

            rt.block_on(async move {
                warp::serve(routes)
                    .bind(addr)
                    .await
                    .graceful(async move {
                        let _ = rx.await;
                    })
                    .run()
                    .await;
            });
        })
        .expect("inspector: spawn thread");

    Arc::new(InspectorHandle::new(tx, th))
}
