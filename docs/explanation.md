# Explanation

## Requirements

This implementation should meet, or at least make provision for meeting, the following criteria:

1. Support for highly concurrent activity
2. Integrate well with the Tokio runtime
3. Ensure good scalability by keeping memory consumption low
4. Expose cache performance and activity statistics (compatible with Prometheus metrics).
5. Integrate well with the `pingora_cache` `EvictionManager` to control the local cache size/population.
6. Allow for a tiered cache architecture: primary cache is the local disk; the secondary cache is a shared or distributed cache layer accessible by multiple servers.
7. Configurable cache behaviour

## 1. High Concurrency Support

Cloudflare are specialists in the area of managing network traffic and distributed connectivity; therefore, the Pingora Framework has been delivered with built-in support for high levels of concurrency.

## 2. Good Tokio Runtime Integration

This is also an off-the-shelf design feature of the Pingora Framework.

## 3. Low Memory Consumption 

The current implementation ensures that memory consumption is kept low by writing cached objects to disk rather than keeping them in memory.

## 4. Expose Cache Performance & Activity Statistics

Currently, the following metrics are kept:

| Metric           | Type      | Description                                                                   |
|------------------|-----------|-------------------------------------------------------------------------------|
| `lookup_hits`    | Monotonic | Cache lookup counter                                                          |
| `served_hits`    | Monotonic | Count of cached object successfully delivered to the client                   |
| `misses`         | Monotonic | Count of cache lookup failures                                                |
| `inserts`        | Monotonic | Count of a new objects added to the cache                                     |
| `purge_attempts` | Monotonic | Incremented each time the `EvictionManager` decides to remove a cached object |
| `evictions`      | Monotonic | Incremented each time a cached object is successfully removed from the cache  |
| `evicted_bytes`  | Monotonic | The total number of bytes removed from the cache                              |
| `size_bytes`     | Variable  | The current size of the cache                                                 |

These matrics are exposed in a format compatible with Prometheus and can be accessed via <http://localhost:8080/metrics>

The actual cache contents can also be accessed via <http://localhost:8080/cache>.
Currently, this a bare-bones interface with no administrative capability; however, it can be expanded to provide an administration interface.

## 5. Integrate well with the `pingora_cache` `EvictionManager`

The Pingora `EvictionManager` is invoked automatically by the Pingora `HttpCache` when it detects that the cache contents need to be altered.
Typically, the eviction manager decides that an object should be removed from the cache because some threshold has been exceeded.

The eviction policy used by the `EvictionManager` is defined at the time the cache is created.
In this demo, the "Least Recently Used" (LRU) policy has been chosen.

```rust
static DEFAULT_CACHE_SIZE_BYTES: &'static usize = &(2 * 1024 * 1024 * 1024); // Default cache size = 2Gb

pub struct EvictCfg {
  pub max_bytes: usize,
}

pub static EVICT_CFG: Lazy<EvictCfg> = Lazy::new(|| EvictCfg {
  max_bytes: env_var_or_num("CACHE_SIZE_BYTES", *DEFAULT_CACHE_SIZE_BYTES),
});
pub static EVICT: Lazy<&'static LruManager> = Lazy::new(|| Box::leak(Box::new(LruManager::new(EVICT_CFG.max_bytes))));
```

Where `LruManager` is an alias for `pingora_cache::eviction::simple_lru::Manager`.

This eviction policy is then applied to the selected cache when `pingora_proxy::ProxyHttp::request_cache_filter()` is called.

## 6. Tiered Cache Architecture

The actual `DiskCaxche` object is declared as follows:

```rust
static DEFAULT_CACHE_DIR: &'static str = "./.cache";

static DISK_CACHE: Lazy<&'static DiskCache> =
    Lazy::new(|| Box::leak(Box::new(DiskCache::new(env_var_or_str("CACHE_DIR", DEFAULT_CACHE_DIR)))));
```

This acts as the primary cached and is passed to the following struct with the option to provide a secondary cache:

```rust
pub struct TieredStorage {
    primary: &'static (dyn Storage + Sync),
    secondary: Option<&'static (dyn Storage + Sync)>,
    write_policy: WritePolicy,
}
```

In order to operate a tiered lookup, the `TieredStorage` struct implements `pingora_cache::Storage` so that when its `lookup` function is called, rather than interacting directly with the disk cache, it first calls `lookup` on the primary cache.
If that fails, it attempts to call `lookup` on the secondary (if one exists).

## 7. Configurable Cache Behaviour

Currently, the following aspects of the cache can be configured at start up by setting the following environment variables:

| Environment Variable | Default Value            | Description                           |
|----------------------|--------------------------|---------------------------------------|
| `CACHE_DIR`          | `./.cache`               | The root directory for the disk cache |
| `CACHE_SIZE_BYTES`   | `2 * 1024 * 1024 * 1024` | Default cache size (2Gb)              |
| `PROXY_HTTP_PORT`    | `6143`                   | Default port for HTTP connections     |
| `PROXY_HTTPS_PORT`   | `6188`                   | Default port for HTTPS connections    |

If any of these environment variables are missing, the default values will be used instead.
In the case of variables holding numeric values, if the value cannot be parsed as a number, then the default will also be used. 

---

# Startup

The `main` function creates lazy static instance of the `DiskCache` called `DISK_CACHE`, which is then used within the `TIERED` cache object.

After this, a proxy is created that implements `pingora_proxy::ProxyHttp`.
Within this proxy is an implementation of the `request_cache_filter` function.
This function is called when the proxy receives an incoming request for an object, and it is within this function that the decision is made to enable (or not) a particular cache for the current request session.

Assuming a cache is enabled for this session (this PoC only has `DISK_CACHE` available), the Pingora Framework then calls the `DiskCache::lookup()` function for the requested resource.
This function determines whether the file is present in the disk cache.
The first request for a resource will always return `None` because we have not yet obtained this object; but a cache hit returns an object that implements `pingora_cache::Storage::HandleHit`.

If the Pingora framework receives `None` from a lookup, it then calls our implementation of the `DiskCache::get_miss_handler` function to obtain an object that implements `HandleMiss`

Either way, hits are handled by a `HitHandler` and misses by a `MissHandler`

# Implementation of the Trait `pingora_cache::Storage`

The `DiskCache` struct implements the trait `pingora_cache::Storage` and acts as the interface between the Pingora Framework and the cached objects stored on disk.
The implemented functions are called automatically by the Pingora Framework as it handles an incoming request for content.

* **`lookup`**<br>
  For a given `CacheKey`, this function examines the storage in which the cached object might be located and returns an appropriate hit handler or `None`.

* **`get_miss_handler`**<br>
  Called if `lookup` returns `None`.
  This function writes the cached object's metadata to the storage and returns a handler that will be called at such time as the body of the requested object arrives.

* **`purge`**<br>
  Called when the `EvictionManager` decides that an object must be removed from the cache.

* **`update_meta`**<br>
  `update_meta` is called to refresh the stored headers/TTL for an object that already exists in storage, but the body has not changed.

* **`as_any`**<br>
  A hook function in which you could cast the cached object to some concrete type.
