// Add the fan_out module when the secondary cache is implemented
// mod fan_out;

use async_trait::async_trait;
// use fan_out::*;
use pingora_cache::{
    key::CompactCacheKey,
    storage::{HitHandler, MissHandler, PurgeType, Storage},
    trace::SpanHandle,
    {CacheKey, CacheMeta},
};
use std::any::Any;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// How writes should propagate when we get an origin MISS
#[derive(Clone, Copy, Debug)]
pub enum WritePolicy {
    /// Only write into the primary local cache (default, simplest)
    PrimaryOnly,
    /// Write into both primary and secondary (best-effort on secondary)
    WriteThroughBoth,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Tiered storage.
///
/// - On `lookup()`: try primary, then secondary. If secondary hits, we serve from it.
///   Maybe the hit from the secondary could be promoted to the primary, but this might confuse the EvictionManager...
/// - On `get_miss_handler()`: by default we write only to primary.
///   The `WritePolicy::WriteThroughBoth` allows for an optional fan out write to secondary.
pub struct TieredStorage {
    primary: &'static (dyn Storage + Sync),
    secondary: Option<&'static (dyn Storage + Sync)>,
    write_policy: WritePolicy,
}

impl TieredStorage {
    pub fn new(
        primary: &'static (dyn Storage + Sync),
        secondary: Option<&'static (dyn Storage + Sync)>,
        write_policy: WritePolicy,
    ) -> Self {
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
        let fn_name = "TieredStorage::lookup()";
        tracing::debug!("---> {fn_name}");

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

        tracing::debug!("<--- {fn_name}");
        Ok(response)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        trace: &SpanHandle,
    ) -> pingora_error::Result<MissHandler> {
        let fn_name = "TieredStorage::get_miss_handler()";
        tracing::debug!("---> {fn_name}");

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

        tracing::debug!("<--- {fn_name}");
        Ok(miss_handler)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        purge_type: PurgeType,
        trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "TieredStorage::purge()";
        tracing::debug!("---> {fn_name}");

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

        tracing::debug!("<--- {fn_name}");
        Ok(existed)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        trace: &SpanHandle,
    ) -> pingora_error::Result<bool> {
        let fn_name = "TieredStorage::update_meta()";
        tracing::debug!("---> {fn_name}");

        // Update primary; best-effort mirror to secondary if present.
        let mut updated = self.primary.update_meta(key, meta, trace).await?;

        if let Some(sec) = self.secondary {
            if let Ok(x) = sec.update_meta(key, meta, trace).await {
                updated |= x;
            }
        }

        tracing::debug!("<--- {fn_name}");
        Ok(updated)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any(&self) -> &(dyn Any + Send + Sync + 'static) {
        self
    }
}
