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

These metrics are exposed in a format compatible with Prometheus and can be accessed via <http://localhost:8080/metrics>

The cache contents can be displayed via a bare-bones display <http://localhost:8080/cache>.

## 5. Integrate well with the Pingora `EvictionManager`

The `EvictionManager` is invoked automatically by the Pingora `HttpCache` when it detects that the cache contents need to be altered.
Typically, the eviction manager decides that an object should be removed from the cache because some time or size threshold has been exceeded.

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

The actual `DiskCache` object is declared as follows and acts as the primary cache.

```rust
static DEFAULT_CACHE_DIR: &'static str = "./.cache";

static DISK_CACHE: Lazy<&'static DiskCache> =
    Lazy::new(|| Box::leak(Box::new(DiskCache::new(env_var_or_str("CACHE_DIR", DEFAULT_CACHE_DIR)))));
```

An instance of the following struct is then created with the `primary` field set equal to the disk cache.
In this case, the `secondary` cache is optional:

```rust
pub struct TieredStorage {
    primary: &'static (dyn Storage + Sync),
    secondary: Option<&'static (dyn Storage + Sync)>,
    write_policy: WritePolicy,
}
```

In order to operate a tiered lookup, the `TieredStorage` struct must also implement `pingora_cache::Storage` so that when its `lookup` function is called, rather than interacting directly with the disk cache, it first calls `lookup` on the primary cache.
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
In the case of variables that should hold numeric values, if the value cannot be parsed as a number, then the default will also be used. 

---

# Startup

The `main` function creates a lazy static instance of `DiskCache` called `DISK_CACHE`, which is then used as the primary cache within the `TIERED` cache object.

After this, a proxy is created that implements `pingora_proxy::ProxyHttp`.
Within this proxy is an implementation of the `request_cache_filter` function.
This function is called when the proxy receives an incoming request for an object, and it is within this function that the decision is made to enable (or not) a particular cache for the current request session.

In other words, should you need it, the Pingora framework provides the flexibility to implement request specific caches.   

Assuming a cache is enabled for the current request session (in this PoC, only the `DISK_CACHE` is available), the Pingora Framework then calls the `DiskCache::lookup()` function for the requested resource.
This function determines whether the file is present in the disk cache.
The first request for a resource will always return `None` because we have not yet obtained this object; but a cache hit returns an object that implements `pingora_cache::Storage::HandleHit`.

If the Pingora framework receives `None` from a lookup, it then calls our implementation of the `DiskCache::get_miss_handler` function to obtain an object that implements `HandleMiss`

Either way, hits are handled by a `HitHandler` and misses by a `MissHandler`

# Implementation of `pingora_proxy::ProxyHttp`

The struct `EdgeCdnProxy` implements the trait `pingora_proxy::ProxyHttp`.
This means it can act as a standard proxy managed by the Pingora Framework.

The following functions need to be implemented:

* ***`new_ctx`***<br>
   Creates a new proxy context.
   This demo does not use a proxy context.

* ***`upstream_peer`***<br>
   The purpose of this function is to calculate how to communicate with the upstream server.
   It does this by examining the contents of the incoming request.

   In this case, it first fetches the HTTP header `Host`. 
   Then it works out how to communicate with the upstream server by first looking for the value of the pseudo-header `:scheme`.
   If this cannot be found, it then looks for the value of the request header `X-Forwarded-Proto`.
   If that cannot found, it drops back to `http`.

   Once these values have been derived, it returns a `pingora_core::upstreams::peer::HttpPeer` that tells Pingora how to communicate with the upstream server.

* ***`request_cache_filter`***<br>
   As long as the request does not create a request feedback loop (I.E. a request aimed at the proxy itself), this function connects the `DISK_CACHE` with the received `session` object.

* ***`cache_key_callback`***<br>
   This function generates a `CacheKey` for the currently requested resource.
   In this demo implementation, the `CacheKey` is generated using only the `primary` value; the `namespace` and `user_tag` parts are not used.

* ***`cache_hit_filter`***<br>
   Pingora calls this function after a successful cache hit and can optionally be used to invalidate a cached resource.

* ***`response_cache_filter`***<br>
   Pingora calls this function to decide if the resource is cacheable.
   
   It is very important to honour the contents of the `cache-control` header and not cache object marked as `no-store`.

   This implementation also arbitrarily refuses to cache objects that are not returned with an HTTP 2xx status code.

* ***`upstream_response_filter`***<br>
  Sets the HTTP header `X-CDN-Cache: MISS` to record the fact that the object was not served from the cache.


* ***`response_filter`***<br>
  Sets the HTTP header `X-CDN-Cache` to `MISS` or `HIT` depending on whether the object was served from the cache.

# Implementation of `pingora_cache::Storage`

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

# Open Questions and Limitations of the Current Implementation

## Tiered Architecture

A tiered cache architecture raises the following questions:

1. If we get a miss from the primary cache, but a hit in the secondary, should that object be promoted to the primary?
   
   If the answer is yes, then the current implementation of `DiskCache` will need to implement its own "admin hook" that permits the insertion of a new object without interfering with or confusing the operation of the `EvictionManager`.

   Pingora does not appear offer a built-in mechanism for object promotion, so one would have to be designed.

2. In order to reduce potential response latency, it might be worth considering whether the extra workload of a hedged lookup is acceptable.

   In other words, we set an arbitrary response time within which the primary cache should respond. 
   If that threshold is exceeded, then we pre-emptively request the object from the secondary cache and then use the object from whichever cache answers first.

   ***CAVEAT:***<br>
   If badly configured, the "primary cache response time" could have a negative impact on cache performance; therefore it would need to be monitored and tuned carefully.
   Consequently, such a value should not be implemented without providing a corresponding ability to adjust it dynamically.

## Metrics at Startup

Cache metrics are not persisted when the server shuts down; therefore, when the server restarts, all the cached object are still present on disk, but the metrics accumulated during the creation of the current cache state have been lost.

## Administrator's Dashboard

What dashboard?  🤣

The current display of the cache contents is a bare-bones implementation that currently offers no administrative tools.
This needs to be implemented.

### Useful Administrative Features

1. Change cache size without having to restart the server.

   This could be achieved by implementing a wrapper around `EvictionManager`, but the actual details need further investigation.

2. Hot Upgrade.

   By wrapping the actual proxy server in an admin wrapper, not only could the administrative features be closely coupled to the server, but an Erlang-style hot upgrade could be implemented 