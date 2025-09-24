use crate::{
    consts::CACHE_STATE_FILENAME,
    disk_cache::DiskCache,
    eviction_manager_cfg,
    utils::{impl_trace, Trace},
};

use async_trait::async_trait;
use pingora_core::{server::ShutdownWatch, services::background::BackgroundService};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufReader, Result},
    path::PathBuf,
    sync::Arc,
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Persisted cache values
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStatistics {
    pub root: PathBuf,
    pub start_time: std::time::SystemTime,
    pub uptime: std::time::Duration,
    pub size_bytes_current: u64,
    pub size_bytes_max: u64,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Persist cache data
pub struct PersistCacheOnShutdown {
    pub cache: Arc<&'static DiskCache>,
}

impl_trace!(PersistCacheOnShutdown);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// When the Pingora server shuts down or upgrades, write the cache statistics to disk
#[async_trait]
impl BackgroundService for PersistCacheOnShutdown {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        <Self as Trace>::fn_enter("start");

        // Wait for graceful shutdown
        // This is only fired for SIGTERM or SIGQUIT, NOT SIGINT
        let _ = shutdown.changed().await;

        let mut path = PathBuf::from(self.cache.root.as_path());
        path.push(CACHE_STATE_FILENAME);

        let cs = CacheStatistics {
            root: self.cache.root.clone(),
            start_time: self.cache.start_time,
            uptime: self.cache.start_time.elapsed().unwrap(),
            size_bytes_current: self.cache.metrics.size_bytes.get() as u64,
            size_bytes_max: eviction_manager_cfg().max_bytes as u64,
        };

        if let Ok(json) = serde_json::to_string_pretty(&cs) {
            let _ = tokio::fs::write(&path, json).await;
        } else {
            tracing::error!("Failed to serialize CacheStatistics to JSON");
        }

        <Self as Trace>::fn_exit("start");
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn fetch_cache_state(root: PathBuf) -> Result<CacheStatistics> {
    let mut path = PathBuf::from(root.as_path());
    path.push(CACHE_STATE_FILENAME);

    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let stats = serde_json::from_reader(reader)?;

    Ok(stats)
}
