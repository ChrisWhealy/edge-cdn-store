# Explanation

## Requirements

This implementation should meet, or at least make provision for meeting, the following criteria:

1. Efficiently support a large amount of concurrent activity
2. Integrate well with the Tokio runtime
3. Ensure good scalability by keeping memory consumption low
3. Expose cache performance and activity statistics (compatible with Prometheus metrics).
4. Integrate well with the `pingora_cache` `EvictionManager` to control the local cache size/population.
5. Allow for a tiered cache architecture: primary cache is the local disk; the secondary cache is a shared or distributed cache layer accessible by multiple servers.
6. Configurable cache behaviour

## Implementation of the Trait `pingora_cache::Storage`

### `DiskCache`

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


### Tiered Cache Architecture

The cache is implemented as `struct DiskCache` that then implements the `pringora_cache::Storage` trait.

The `main` function creates lazy static instance of the `DiskCache` called `DISK_Cache`, which is then used within the `TIERED` cache object.
(Currently, there is only one cache, but the `TEIRED` object allows for there to be a secondary, remote cache to act as a fallback if the we get a miss on a local cache lookup)

After this, a proxy is created that implements `pingora_proxy::ProxyHttp`.
Within this proxy is an implementation of the `request_cache_filter` function.
This function is called when the proxy receives an incoming request for an object, and it is within this function that the decision is made to enable (or not) a particular cache for the current session.

Assuming a cache is enabled for this session (this PoC only has `DISK_CACHE` available), the Pingora Framework then calls the `DiskCache::lookup()` function for the requested resource.

Our implementation of this function then determines whether the file is present in the disk cache.
The first request for a resource will always return `None` because we have not yet obtained this object; but a cache hit returns an object that implements `pingora_cache::Storage::HandleHit`.

If the Pingora framework receives `None` from a lookup, it then calls our implementation of the `DiskCache::get_miss_handler` function to obtain an object that implements `HandleMiss`

Either way, hits are handled by a `HitHandler` and misses by a `MissHandler`

As hits, misses, purge attempts and evictions on cached objects happen, the appropriate metrics are recorded and made available through the cache inspection enpoint running on http://localhost:8080/matrics.
This information is made available in a form that can be consumed by Prometheus.
