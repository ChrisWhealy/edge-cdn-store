# WARP Proposal

## Introduction

Any time a person uses a computer to access information over the Worldwide Web, buy something from an online vendor, or perform some sort of productivity task (such as writing a document, using a shared calendar or creating a business document), they will, mostly likely, perform that task using a Web Browser.

However, the machine on which the Web Browser (the client) runs is frequently located at a large physical distance from the Web Server with which they are interacting.
This in turns means that the data involved in each request/response cycle must travel through a potentially large number of network switches, routers and servers before completing its round trip.

It is therefore self-evident that the fewer intermediate steps there are in this "_journey through the network_", the quicker the request/response cycle can be completed.

In simplistic terms, the request/response cycle between a Web Browser and a Web Server looks something like this:

```mermaid
sequenceDiagram
    participant Browser
    participant Web Server
    
    Note over Browser, Web Server: User enters a website address
    Browser->>Web Server: Hey website, send me your first page
    Web Server-->>Browser: OK, here you go
    
    Note over Browser, Web Server: Browser examines page content<br>and makes further requests
    Browser->>Web Server: So I also need some image files, some style<br>sheets and a few JavaScript programs
    Web Server-->>Browser: Ok, here are the images
    Web Server-->>Browser: and the style sheets
    Web Server-->>Browser: and the JavaScript...
    
    Note over Browser, Web Server: User interacts with the web page
    Browser->>Web Server: Now I need another web page with<br>some more images and stylesheets
    Web Server-->>Browser: Ok, here are the images
    Web Server-->>Browser: Hang on, didn't I just send you that style sheet? 
```

Whilst all browsers operate their own local cache to avoid requesting a resource they have already been sent, it is not always possible for a browser to recognise that it is requesting the same resource.

This might be because the URL pointing to a particular resource might use a dynamically generated path (or even file) name which changes between visits or between user sessions: yet the actual resource behind the request remains the same.

## Motivation

### Problem Description

If a user located in Singapore is interacting with a website hosted in a data centre on the US East Coast, they may experience a noticeable delay between clicking on a link and seeing the response in their browser.

This is due simply to the fact that the Worldwide Web is exactly that - worldwide.
Consequently, if two computers on opposite sides of the planet wish to talk to each other, their requests and responses must traverse a complex network topology and pass through a large number of intervening switches, routers and servers in order to complete a single round trip.

Users will tolerate a certain amount of delay at certain points in the interaction sequence, but if that delay is either sufficiently frequent or becomes sufficiently large, then they will typically become frustrated and eventually stop using the website.

### Proposed Benefits

This proposal aims to reduce the network round trip time by reducing the "_network distance_" between the client and server.
So rather than the server being located deep within a network (with all the accompanying traffic delays needed for the request to get in and the response to get out), the request can be answered by a server located very close to, or even on, the "edge" of the network.

So in the case of our user in Singapore, if this "edge-cache" software were installed on servers running in a local Singapore data centre, then they would experience much faster response times simply because the "_network distance_" between client and server is shorter.

Software that implements this "edge-cache" functionality is part of the wider category of software forming what is called a Content Delivery Network (CDN). 

Wasmer Edge is the name of Wasmer's content delivery network (CDN) offering and this proposal aims to extend its functionality by adding CDN caching capability.

### Impact

From a technical perspective, to implement a correct, scalable and robust cache layer from scratch is a highly complex task requiring both specialist skills and an extended period of development time.

From an entirely pragmatic perspective, this is likely beyond the reach of a small company such as Wasmer - and even if a home-grown product were somehow to be built successfully, if that product could not demonstrate at least feature parity with existing CDN offerings, then its presence risks undermining customer confidence.

Consequently, it makes sense for Wasmer to use Open Source Rust software delivered by a CDN specialist company such as Cloudflare as they have already done the technical "heavy lifting".

Such an approach allows Wasmer to implement the required functionality at a lower cost and results in a much smaller operational burden.

Cloudflare's Rust-based Pingora framework is a candidate here; specifically, the `pingora-proxy` and `pingora-cache` crates.

From a product perspective however, even after an edge-cache has been built using this software, this may only bring Wasmer up to basic parity with other CDN offerings.
In order to make the Wasmer Edge product stand out, further usability features will be needed that could include:
* A dashboard that allows admin tasks to be performed such as monitoring cache performance and manual cache purges etc
* Perform the same tasks as above but programmatically via an API
* Other features I haven't thought of yet

But first things first&hellip;

## Explanation

Explanation of the proposal.

This should be either short and broad for exploratory WARPs, or very detailed, depending on the context.

## Drawbacks & Alternatives

The main downside of implementing this proposal is that although Cloudflare state that the [Pingora is battle tested...](https://github.com/cloudflare/pingora?tab=readme-ov-file#what-is-pingora), they also explicitly state that [Pingora proxy integration with caching should be considered experimental](https://github.com/cloudflare/pingora?tab=readme-ov-file#notable-crates-in-this-workspace), and as such APIs related to caching are currently highly volatile.

Looking through the `pingora-cache` repository, the `memory` module is specifically identified as [not being production ready](https://github.com/cloudflare/pingora/blob/b3c186177e8ff59f047ed05aa7b88735bb623c2f/pingora-cache/src/memory.rs#L17).
Although none of the other modules contain such an explicit warning, this offers no guarantee that volatility will be confined simply to this one module.

Given Cloudflare's warning, for Wasmer to build some mission-critical software on a foundation known to be volatile should be considered ***high risk***.

These risks include, but are not limited to, building mission-critical functionality on top of a `pingora-cache` feature that:
* is substantially altered or maybe even disappear as part of a minor release; or
* turns out to be inefficient or even buggy

Wasmer could end up in a situation in which part of their mission-critical Wasmer Edge offering is dependent upon a crate version of `pingora-cache` that cannot be upgraded without significant effort or rework.

### Alternatives

***Commercial Products***<br>
Commercial CDN cache products are also available, but each comes with some variation of volume-based pricing:
* <https://bunny.net/pricing>
* <https://keycdn.com/pricing> (Focussed on Europe)
* <https://www.cdn77.com/pricing>

The chief risks of building edge-cache functionality on top of a paid-for product are these:
* The ongoing costs payable to the third-party will
  * increase as Wasmer's user base (and therefore throughput) grows
  * become a unavoidable running cost that cannot be reduced without significant effort  
* In future, should it become necessary to detach Wasmer Edge from such third-party dependencies, then further development time and effort must be spent detaching from one cache solution and then transitioning to another cache solution - all without disrupting the existing functionality.

***Open Source Alternatives***<br>
Alternative OSS caching proxies are available that include:

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
1. Varnish is controlled using either HTTP, its own API or its Control Interface (CLI).  This therefore incurs at least one extra network hop
1. Rust calls to the Varnish API should be done using a separate thread pool (`tokio::task::spawn_blocking`)
1. In order to maintain consistent observability, Varnish's metrics (collected using `Varnishstat`) would have to be integrated with whatever metrics pipeline the Rust application uses
1. Increased operational burden due to Varnish having its own configuration, tuning and patching requirements
1. Implementing more fine-grained controls such as cache purge by header, or tenant isolation can be done, but the Rust proxy must interact with Varnish's CLI
1. Debugging will be harder because both the Rust and Varnish (C) sides of the functionality will need to be traced.

[↩](#a1)