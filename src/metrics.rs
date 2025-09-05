use prometheus::{register_int_counter, register_int_gauge, IntCounter, IntGauge};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct CacheMetrics {
    pub lookup_hits: IntCounter,
    pub served_hits: IntCounter,
    pub misses: IntCounter,
    pub inserts: IntCounter,
    pub purge_attempts: IntCounter,
    pub evictions: IntCounter,
    pub evicted_bytes: IntCounter,
    pub size_bytes: IntGauge,
}

impl CacheMetrics {
    pub fn new() -> Self {
        Self {
            lookup_hits: register_int_counter!("cache_lookup_hits", "Cache lookup hits").unwrap(),
            served_hits: register_int_counter!("cache_served_hits", "Cache served hits").unwrap(),
            misses: register_int_counter!("cache_misses", "Cache misses").unwrap(),
            inserts: register_int_counter!("cache_inserts", "Cache insertions").unwrap(),
            purge_attempts: register_int_counter!("purge_attempts", "Purge attempts").unwrap(),
            evictions: register_int_counter!("cache_evictions", "Successful cache evictions").unwrap(),
            evicted_bytes: register_int_counter!("evicted_bytes", "Total bytes evicted").unwrap(),
            size_bytes: register_int_gauge!("cache_size_bytes", "Current cache size in bytes").unwrap(),
        }
    }
}
