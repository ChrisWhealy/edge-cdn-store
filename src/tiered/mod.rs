// Add the fan_out module when the secondary cache is implemented
// mod fan_out;

use crate::{
    disk_cache::disk_cache,
    utils::{impl_trace, Trace},
};

use async_trait::async_trait;
// use fan_out::*;
use pingora_cache::{
    key::CompactCacheKey, storage::{HitHandler, MissHandler, PurgeType, Storage},
    trace::SpanHandle,
    CacheKey,
    CacheMeta,
};
use std::{any::Any, sync::OnceLock};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
static TIERED: OnceLock<TieredStorage> = OnceLock::new();
pub fn tiered_cache() -> &'static  TieredStorage {
    TIERED.get_or_init(|| TieredStorage::new(
            disk_cache(),
            None,
            // Some(remote_cache()),  // Need to implement this
            WritePolicy::PrimaryOnly, // Switches to WriteThroughBoth when remote is available
        )
    )
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// How writes should propagate when we get an origin MISS
#[derive(Clone, Copy, Debug)]
pub enum WritePolicy {
    /// Only write into the primary local cache (default, simplest)
    PrimaryOnly,
    #[allow(dead_code)]
    /// Write into both primary and secondary (best-effort on secondary)
    WriteThroughBoth,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Tiered storage.
///
/// - On `lookup()`: try primary, then secondary. If secondary hits, we serve from it.
///   Maybe the hit from the secondary could be promoted to the primary, but that would need to be done in a way that
///   does not confuse the EvictionManager...
/// - On `get_miss_handler()`: by default we write only to primary.
///   The `WritePolicy::WriteThroughBoth` allows for an optional fan out write to secondary.
pub struct TieredStorage {
    pub primary: &'static (dyn Storage + Sync),
    pub secondary: Option<&'static (dyn Storage + Sync)>,
    pub write_policy: WritePolicy,
}

impl_trace!(TieredStorage);

impl TieredStorage {
    pub fn new(
        primary: &'static (dyn Storage + Sync),
        secondary: Option<&'static (dyn Storage + Sync)>,
        write_policy: WritePolicy,
    ) -> Self {
        <Self as Trace>::fn_enter_exit("new");
        Self {
            primary,
            secondary,
            write_policy,
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl Storage for TieredStorage {
    async fn lookup(
        &'static self,
        key: &CacheKey,
        trace: &SpanHandle,
    ) -> pingora_error::Result<Option<(CacheMeta, HitHandler)>> {
        let fn_name = "lookup";
        <Self as Trace>::fn_enter(fn_name);

        let response: Option<(CacheMeta, HitHandler)> = if let Some(hit) = self.primary.lookup(key, trace).await? {
            // Response from primary
            Some(hit)
        } else if let Some(secondary) = self.secondary {
            if let Some(hit) = secondary.lookup(key, trace).await? {
                // Response from secondary
                Some(hit)
            } else {
                None
            }
        } else {
            None
        };

        <Self as Trace>::fn_exit(fn_name);
        Ok(response)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        trace: &SpanHandle,
    ) -> pingora_error::Result<MissHandler> {
        let fn_name = "get_miss_handler";
        <Self as Trace>::fn_enter(fn_name);

        // Always create a primary miss handler
        let primary_miss = self.primary.get_miss_handler(key, meta, trace).await?;

        let miss_handler = match (self.write_policy, self.secondary) {
            (WritePolicy::PrimaryOnly, _) | (_, None) => {
                // Simple: only admit into primary
                primary_miss
            },
            (WritePolicy::WriteThroughBoth, Some(_sec)) => {
                // TODO implement some sort of best-effort write through to secondary
                // At the moment, changing the WriteThroughPolicy makes no difference
                primary_miss
            },
        };

        <Self as Trace>::fn_exit(fn_name);
        Ok(miss_handler)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        purge_type: PurgeType,
        trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "purge";
        <Self as Trace>::fn_enter(fn_name);

        let existed = match purge_type {
            PurgeType::Eviction => {
                // Eviction happens because the primary is under pressure
                // Don't touch the secondary
                self.primary.purge(key, purge_type, trace).await.unwrap_or_default()
            },
            PurgeType::Invalidation => {
                // Invalidation requires synchronised behaviour across the primary and secondary
                let mut existed = false;
                match self.primary.purge(key, purge_type, trace).await {
                    Ok(x) => existed |= x,
                    Err(e) => tracing::warn!("primary purge failed during invalidation: {e}"),
                }
                if let Some(sec) = self.secondary {
                    match sec.purge(key, purge_type, trace).await {
                        Ok(x) => existed |= x,
                        Err(e) => tracing::warn!("secondary purge failed during invalidation: {e}"),
                    }
                }

                existed
            },
        };

        <Self as Trace>::fn_exit(fn_name);
        Ok(existed)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "update_meta";
        <Self as Trace>::fn_enter(fn_name);

        // Update primary; best-effort mirror to secondary if present.
        let mut updated = self.primary.update_meta(key, meta, trace).await?;

        if let Some(sec) = self.secondary {
            if let Ok(x) = sec.update_meta(key, meta, trace).await {
                updated |= x;
            }
        }

        <Self as Trace>::fn_exit(fn_name);
        Ok(updated)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any(&self) -> &(dyn Any + Send + Sync + 'static) {
        self
    }
}
