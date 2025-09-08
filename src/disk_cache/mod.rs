mod handle_hit;
mod handle_miss;
pub mod inspector;

use crate::{
    disk_cache::{handle_hit::DiskHitHandler, handle_miss::DiskMissHandler},
    metrics::CacheMetrics,
    utils::{format_cache_key, trace_fn_exit_with_err},
};

use async_trait::async_trait;
use pingora_cache::{
    key::{CacheHashKey, CompactCacheKey},
    storage::{HitHandler, MissHandler, PurgeType, Storage},
    trace::SpanHandle,
    {CacheKey, CacheMeta},
};
use std::{
    any::Any,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// The cache directory structure is as follows, with each part of the cached data living in its own directory
///
/// The value of `hash` is provided by Pingora
///
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/body`
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/meta`
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/hdr`
#[derive(Clone)]
pub struct DiskCache {
    pub root: PathBuf,
    pub metrics: Arc<CacheMetrics>,
}

impl DiskCache {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        tracing::debug!("<--> DiskCache::new() at {}", root.as_ref().display());
        Self {
            root: root.as_ref().to_path_buf(),
            metrics: Arc::new(CacheMetrics::new()),
        }
    }

    fn path_from_key(&self, key: &CacheKey) -> (String, PathBuf, PathBuf, PathBuf, PathBuf) {
        self.path_from_hash(key.combined())
    }

    fn path_from_compact_key(&self, key: &CompactCacheKey) -> (String, PathBuf, PathBuf, PathBuf, PathBuf) {
        self.path_from_hash(key.combined())
    }

    fn path_from_hash(&self, hash: String) -> (String, PathBuf, PathBuf, PathBuf, PathBuf) {
        let dir = self.root.join(&hash[0..2]).join(&hash[2..4]).join(hash.clone());
        let body = dir.join("body");
        let meta = dir.join("meta");
        let hdr = dir.join("hdr");

        (hash, dir, body, meta, hdr)
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl Storage for DiskCache {
    async fn lookup(
        &'static self,
        key: &CacheKey,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<Option<(CacheMeta, HitHandler)>> {
        let fn_name = "DiskCache::lookup()";
        tracing::debug!("---> {fn_name}");

        if let Err(e) = fs::create_dir_all(&self.root).await {
            return trace_fn_exit_with_err(
                fn_name,
                &format!("Unable to create disk cache directory {}: {}", self.root.display(), e),
                false,
            );
        };

        let (_hash, _dir, body_path, meta_path, hdr_path) = self.path_from_key(key);

        Ok(
            match (
                fs::read(&meta_path).await.ok(),
                fs::read(&hdr_path).await.ok(),
                fs::read(&body_path).await.ok(),
            ) {
                // Require meta + body + hdr to exist
                (Some(meta_bin), Some(hdr_bin), Some(body)) => {
                    tracing::debug!("     Cache hit on key {}", format_cache_key(key));
                    self.metrics.lookup_hits.inc();

                    // Deserialize meta and build hit handler
                    let meta = CacheMeta::deserialize(&meta_bin, &hdr_bin)?;
                    let body_len = body.len();
                    let hit = DiskHitHandler {
                        body: Arc::new(body),
                        done: false,
                        range_start: 0,
                        range_end: body_len,
                        metrics: self.metrics.clone(),
                    };

                    tracing::debug!("<--- {fn_name}");
                    Some((meta, Box::new(hit)))
                },
                _ => {
                    self.metrics.misses.inc();
                    tracing::debug!("     Cache miss");
                    tracing::debug!("<--- {fn_name}");
                    None
                },
            },
        )
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<MissHandler> {
        let fn_name = "DiskCache::get_miss_handler()";
        tracing::debug!("---> {fn_name}");
        self.metrics.misses.inc();

        let (hash, dir, body_path, meta_path, hdr_path) = self.path_from_key(key);

        // Serialize meta but do NOT write yet — only commit at finish()
        let (meta_internal, meta_header) = meta.serialize()?;

        // Prepare a temp file for the body
        tracing::debug!("     creating tmp cache dir");
        fs::create_dir_all(&dir).await.ok(); // best-effort
        let (tmp_path, _file) = DiskMissHandler::create_tmp(&dir, &hash).await?;

        let disk_miss_handler = DiskMissHandler {
            tmp_path,
            tmp_bytes_written: 0,
            dir,
            body_path,
            meta_path,
            hdr_path,
            meta_internal,
            meta_header,
            metrics: self.metrics.clone(),
        };

        tracing::debug!("<--- {fn_name}");
        Ok(Box::new(disk_miss_handler))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        _purge_type: PurgeType,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "DiskCache::purge()";
        tracing::debug!("---> {fn_name}");

        self.metrics.purge_attempts.inc();

        let (_hash, dir, body_path, meta_path, hdr_path) = self.path_from_compact_key(key);
        let body_bytes = if let Ok(md) = tokio::fs::metadata(&body_path).await { md.len() } else { 0u64 };

        let existed = match tokio::fs::remove_file(&body_path).await {
            Ok(()) => {
                tracing::debug!("     Purged {body_bytes} bytes");
                self.metrics.evictions.inc();
                self.metrics.evicted_bytes.inc_by(body_bytes);
                self.metrics.size_bytes.sub(body_bytes as i64);
                true
            },
            // Might need to do something else here...
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            // Strictly speaking, setting existed = false here might not be correct
            Err(_) => false,
        };

        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_file(&hdr_path);
        let _ = std::fs::remove_dir(&dir); // Ignore possible error due to races with above fs_remove() calls

        tracing::debug!("<--- {fn_name}");
        Ok(existed)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "DiskCache::update_meta()";
        tracing::debug!("---> {fn_name}");

        let (_hash, _dir, body_path, meta_path, hdr_path) = self.path_from_key(key);

        // The body belonging to this metadata must exist
        let exists = if body_path.exists() {
            tracing::debug!("     body exists");
            let (meta_internal, meta_header) = meta.serialize()?;

            tracing::debug!("     updating meta");
            if let Err(e) = fs::write(&meta_path, &meta_internal).await {
                return trace_fn_exit_with_err(fn_name, &format!("failed to update meta: {e}"), false);
            }

            tracing::debug!("     updating hdr");
            if let Err(e) = fs::write(&hdr_path, &meta_header).await {
                return trace_fn_exit_with_err(fn_name, &format!("failed to update hdr: {e}"), false);
            }

            true
        } else {
            tracing::debug!("     body does not exist");
            false
        };

        tracing::debug!("<--- {fn_name}");
        Ok(exists)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any(&self) -> &(dyn Any + Send + Sync + 'static) {
        self
    }
}
