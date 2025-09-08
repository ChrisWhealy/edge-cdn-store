# Drawbacks & Alternatives

The main downside of implementing this proposal is that although Cloudflare state that [Pingora is battle tested...](https://github.com/cloudflare/pingora?tab=readme-ov-file#what-is-pingora), they also explicitly state that [Pingora proxy integration with caching should be considered experimental](https://github.com/cloudflare/pingora?tab=readme-ov-file#notable-crates-in-this-workspace), and as such APIs related to caching are currently highly volatile.

Looking through the `pingora-cache` repository, the `memory` module is specifically identified as [not being production ready](https://github.com/cloudflare/pingora/blob/b3c186177e8ff59f047ed05aa7b88735bb623c2f/pingora-cache/src/memory.rs#L17).
Although none of the other modules contain such an explicit warning, this offers no guarantee that volatility will be confined simply to the `memory` module.

Given Cloudflare's warning, the risks associated with building mission-critical software on a foundation known to be volatile should be evaluated.
If these risk are considered acceptable, then the following possibilities must be accepted. Wasmer Edge may find itself depending on functionality that:
* is substantially altered or even disappears as part of a future minor release; or
* turns out to be inefficient or even buggy; or
* makes the possibility of an upgrade difficult without significant effort or rework.

## Alternatives

### Commercial Products

[Numerous commercial CDN cache products](https://www.streamingmediablog.com/2023/01/cdn-list.html) are available, of which a tiny selection is listed here, and all of which use some variation of volume-based pricing:

* <https://bunny.net/pricing>
* <https://keycdn.com/pricing> (Focussed on Europe)
* <https://www.cdn77.com/pricing>

The chief risks of building edge-cache functionality on top of a paid-for product are these:
* The ongoing costs payable to a third party will
    * increase as Wasmer's user base (and therefore throughput) grows
    * become a unavoidable running cost that cannot be reduced without significant effort
* In future, should it become necessary to detach Wasmer Edge from such third-party dependencies, then further development time and effort must be spent detaching from one cache solution and then transitioning to another cache solution - all without disrupting the existing functionality.

### Open Source Alternatives

If they have not already done so, Wasmer would be prudent to consider some of the alternative OSS caching proxies solutions.
These include:

* [Varnish HTTP Cache](https://varnish-cache.org/)

  ✅ Widely used by key players such as Akamai and Fastly<br>
  ❌ Written in C not async Rust, so carries a set of interoperability challenges <sup id="a1">[1](#f1)</sup>

* [Apache Traffic Server](https://trafficserver.apache.org/)

  ✅ Offers large-scale distributed caching and advanced routing<br>
  ❌ A large product with a steep learning curve

* [Nginx + `proxy_cache`](https://nginx.org/index.html)

  ✅ Both the free and paid-for versions are widely used and well known<br>
  ❌ The `proxy_cache` module available in the free version offers only basic cache capability.

  To get more advanced features that would differentiate Wasmer Edge as a product from other edge caches, the paid-for version would need to be used.

* [SOZU Proxy](https://www.sozu.io/)

  ✅ Written in async Rust with an Erlang-style always-on, hot-reload architecture<br>
  ❌ Not as mature and therefore not as battle-hardened

---

<b id="f1">1</b>&nbsp;&nbsp; Implementing Varnish from async Rust certainly can be done, but it comes with the following extra considerations:
1. Varnish would probably need to be run as a sidecar per Wasmer Edge node.  This is more memory hungry, but keeps the implementation simpler.
1. Varnish is controlled using either HTTP, its own API or its Control Interface (CLI).  This therefore incurs at least one extra network hop which in turn increases latency.
1. Rust calls to the Varnish API should be done using a separate thread pool (`tokio::task::spawn_blocking`)
1. In order to maintain consistent observability, Varnish's metrics (collected using `Varnishstat`) would have to be integrated with whatever metrics pipeline Wasmer Edge uses
1. Increased operational burden due to Varnish having its own configuration, tuning and patching requirements
1. Implementing more fine-grained controls such as cache purge by header, or tenant isolation can be done, but the Rust proxy can only do so via Varnish's CLI
1. Debugging will be harder because both the Rust and Varnish (C) sides of the functionality will need to be traced.

[↩](#a1)