pub(crate) mod cache_statistics;
mod handle_hit;
mod handle_miss;
pub mod inspector;

use crate::{
    disk_cache::{cache_statistics::fetch_cache_state, handle_hit::DiskHitHandler, handle_miss::DiskMissHandler},
    metrics::CacheMetrics,
    statics::{cache_dir, DEFAULT_CACHE_SIZE_BYTES, DEFAULT_READ_BUFFER_SIZE},
    utils::{env_var_or_num, env_var_or_str},
    utils::{format_cache_key, impl_trace, trace_fn_exit_with_err, Trace},
};

use async_trait::async_trait;
use pingora_cache::{
    eviction::simple_lru::Manager as LruManager, key::{CacheHashKey, CompactCacheKey},
    storage::{HitHandler, MissHandler, PurgeType, Storage},
    trace::SpanHandle,
    CacheKey,
    CacheMeta,
};
use std::{
    any::Any,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, Arc, OnceLock},
};
use tokio::{fs, fs::File, join};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Define cache and eviction policy
static DISK_CACHE: OnceLock<DiskCache> = OnceLock::new();
pub fn disk_cache() -> &'static DiskCache {
    DISK_CACHE.get_or_init(|| DiskCache::new(env_var_or_str("CACHE_DIR", cache_dir())))
}

// Just use LRU at the moment
pub struct EvictionManagerCfg {
    pub max_bytes: usize,
}

static EVICTION_MANAGER_CFG: OnceLock<EvictionManagerCfg> = OnceLock::new();
pub fn eviction_manager_cfg() -> &'static EvictionManagerCfg {
    EVICTION_MANAGER_CFG.get_or_init(|| EvictionManagerCfg {
        max_bytes: env_var_or_num("CACHE_SIZE_BYTES", *DEFAULT_CACHE_SIZE_BYTES),
    })
}

static EVICTION_MANAGER: OnceLock<LruManager> = OnceLock::new();
pub fn eviction_manager() -> &'static LruManager {
    EVICTION_MANAGER.get_or_init(|| LruManager::new(eviction_manager_cfg().max_bytes))
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// The cache directory structure is as follows, with each part of the cached data living in its own directory
///
/// The value of `hash` is provided by Pingora
///
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/body`
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/meta`
///   * `$CACHE_ROOT/hash[0..2]/hash[2..4]/hash/hdr`
pub struct DiskCache {
    pub root: PathBuf,
    pub start_time: std::time::SystemTime,
    #[allow(dead_code)]
    pub uptime: AtomicU64,
    pub metrics: Arc<CacheMetrics>,
}

impl_trace!(DiskCache);

impl DiskCache {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        <Self as Trace>::fn_enter_exit("new");

        let prev_size = if let Ok(stats) = fetch_cache_state(root.as_ref().to_path_buf()) {
            tracing::debug!("Fetched existing cache statistics: {:#?}", stats);
            stats.size_bytes_current as i64
        } else {
            tracing::debug!("Previous cache statistics not found in {}", root.as_ref().display());
            0
        };

        // Has the maximum cache size shrunk as a result of this restart?
        if eviction_manager_cfg().max_bytes < prev_size as usize {
            tracing::warn!("Maximum cache size is now smaller than the current cache size on disk!")
        }

        Self {
            root: root.as_ref().to_path_buf(),
            start_time: std::time::SystemTime::now(),
            uptime: AtomicU64::new(0),
            metrics: Arc::new(CacheMetrics::new(prev_size)),
        }
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn path_from_key(&self, key: &CacheKey) -> (String, PathBuf, PathBuf, PathBuf, PathBuf) {
        self.path_from_hash(key.combined())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn path_from_compact_key(&self, key: &CompactCacheKey) -> (String, PathBuf, PathBuf, PathBuf, PathBuf) {
        self.path_from_hash(key.combined())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
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
        let fn_name = "lookup";
        <Self as Trace>::fn_enter(fn_name);

        if let Err(e) = fs::create_dir_all(&self.root).await {
            return trace_fn_exit_with_err(
                fn_name,
                &format!("Unable to create disk cache directory {}: {}", self.root.display(), e),
                false,
            );
        };

        let (_hash, _dir, body_path, meta_path, hdr_path) = self.path_from_key(key);

        // Read only those files we know are small
        let (meta_res, hdr_res, file_res) = join!(
            fs::read(&meta_path),
            fs::read(&hdr_path),
            // TODO: need to look into how this file descriptor can be shared between requests
            File::open(&body_path)
        );

        let result = match (meta_res, hdr_res, file_res) {
            (Ok(meta_bin), Ok(hdr_bin), Ok(file)) => {
                // determine total length once
                let total_len = match file.metadata().await {
                    Ok(m) => m.len(),
                    Err(e) => {
                        return trace_fn_exit_with_err(
                            fn_name,
                            &format!("unable to determine file size for cached entry {}: {e}", body_path.display()),
                            false,
                        );
                    },
                };

                self.metrics.lookup_hits.inc();
                tracing::debug!("     Cache hit on key {}", format_cache_key(key));

                let meta = CacheMeta::deserialize(&meta_bin, &hdr_bin)?;
                let hit = DiskHitHandler {
                    file,
                    total_len,
                    range_start: 0,
                    range_end: total_len,
                    pos: 0,
                    pending_seek: false, // defer seek
                    chunk: DEFAULT_READ_BUFFER_SIZE,
                    metrics: self.metrics.clone(),
                };

                Some((meta, Box::new(hit) as HitHandler))
            },
            _ => {
                self.metrics.misses.inc();
                tracing::debug!("     Cache miss");
                None
            },
        };

        <Self as Trace>::fn_exit(fn_name);
        Ok(result)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<MissHandler> {
        let fn_name = "get_miss_handler";
        <Self as Trace>::fn_enter(fn_name);
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

        <Self as Trace>::fn_exit(fn_name);
        Ok(Box::new(disk_miss_handler))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        _purge_type: PurgeType,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "purge";
        <Self as Trace>::fn_enter(fn_name);

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
            Err(e) if e.kind() == ErrorKind::NotFound => false,
            // Strictly speaking, setting existed = false here might not be correct
            Err(_) => false,
        };

        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_file(&hdr_path);
        let _ = std::fs::remove_dir(&dir); // Ignore possible error due to races with above fs_remove() calls

        <Self as Trace>::fn_exit(fn_name);
        Ok(existed)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "update_meta";
        <Self as Trace>::fn_enter(fn_name);

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

        <Self as Trace>::fn_exit(fn_name);
        Ok(exists)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any(&self) -> &(dyn Any + Send + Sync + 'static) {
        self
    }
}
