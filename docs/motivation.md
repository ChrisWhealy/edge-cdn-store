# Motivation

## Problem Description

If a user located in Singapore is interacting with a website hosted in a data centre on the US East Coast, they may experience a noticeable delay between clicking on a link and seeing the response in their browser.

This is due simply to the fact that the Worldwide Web is exactly that - worldwide.
Consequently, if two computers on opposite sides of the planet wish to talk to each other, their requests and responses must traverse a complex network topology and pass through a large number of intervening switches, routers and servers in order to complete a single round trip.

Users will tolerate a certain amount of delay at certain points in the interaction sequence, but if that delay is either sufficiently frequent or becomes sufficiently large, then they will typically become frustrated and eventually stop using the website.

## Proposed Benefits

This proposal aims to reduce the network round trip time by reducing the "_network distance_" between the client and server.
So rather than the server being located deep within a network (with all the accompanying traffic delays needed for the request to get in and the response to get out), the request can be answered by a server located very close to, or even on, the "edge" of the network.

So in the case of our user in Singapore, if their requests are routed through servers running in a local Singapore data centre, and those servers are also running this "edge-cache" software, then the user would experience much faster response times simply because the "_network distance_" between the client and the server is shorter.

Software that implements this "edge-cache" functionality is part of the wider category of software forming what is called a Content Delivery Network (CDN).

Wasmer Edge is the name of Wasmer's CDN offering and this proposal aims to extend its functionality by adding edge-caching capability.

## Impact

From a technical perspective, to implement a correct, scalable and robust cache layer from scratch is a highly complex task requiring both specialist skills and an extended period of development time.
Consequently, it makes sense for Wasmer to use an Open Source Rust framework delivered by a company specialising in CDN software as they have expertise to perform the technical "heavy lifting".

Cloudflare's Rust-based Pingora framework is a candidate here; specifically, the `pingora-proxy` and `pingora-cache` crates.

From a product perspective however, even after an edge-cache has been built using this software, this may only bring Wasmer up to basic parity with other CDN offerings.
In order to make the Wasmer Edge product stand out, further usability features will be needed that could include:
* A dashboard that allows admin tasks to be performed such as monitoring cache performance and manual cache purges etc
* Perform the same tasks as above but programmatically via an API
* Other features I haven't thought of yet
