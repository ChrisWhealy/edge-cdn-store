use crate::{disk_cache::DiskCache, CACHE_STATE_FILENAME};

use serde::{Deserialize, Serialize};
use std::{
    io::{BufReader, BufWriter, Error, ErrorKind},
    path::PathBuf,
    sync::atomic::Ordering,
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Persist cache data
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStatistics {
    pub root: PathBuf,
    pub start_time: std::time::SystemTime,
    pub uptime: std::time::Duration,
    pub size_bytes: u64,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn persist_cache_state(cache: &'static DiskCache) -> std::io::Result<()> {
    let mut path = PathBuf::from(cache.root.as_path());
    path.push(CACHE_STATE_FILENAME);

    let file = std::fs::File::create(path)?;
    let writer = BufWriter::new(file);
    let cs = CacheStatistics {
        root: cache.root.clone(),
        start_time: cache.start_time,
        uptime: std::time::Duration::from_secs(cache.uptime.load(Ordering::Relaxed)),
        size_bytes: cache.metrics.size_bytes.get() as u64,
    };

    serde_json::to_writer_pretty(writer, &cs).map_err(|e| Error::new(ErrorKind::Other, e))?;

    Ok(())
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn fetch_cache_state(root: PathBuf) -> std::io::Result<CacheStatistics> {
    let mut path = PathBuf::from(root.as_path());
    path.push(CACHE_STATE_FILENAME);

    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let stats = serde_json::from_reader(reader)?;

    Ok(stats)
}
