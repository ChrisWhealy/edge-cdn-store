# PoC Edge CDN Cache

A CDN Edge reverse proxy PoC based on Cloudflare's [Pingora Framework](https://github.com/cloudflare/pingora).

The proposal is described [here](docs/README.md)

## Generate TLS Certificate

This PoC requires that your server certificate and private key files are located in the repo's `./keys` directory as `server.crt` and `server.pem`.

## Usage

The server will start up using the value in the environment variable `EDGE_RUNTIME_DIR` as its runtime directory.
If this variable is unset or empty, the runtime directory defaults to `/tmp/edge-cdn-store`.

All cached responses are stored in the directory `$EDGE_RUNTIME_DIR/cache`.

### Start server

```bash
./start.sh edge-cdn-store.yaml
```

This starts the Edge CDN Store proxy as a background task that makes the following three endpoints available:

* `http://localhost:6188` Insecure (HTTP) proxy endpoint
* `https://localhost:6143` TLS (HTTPS) proxy endpoint
* `http://localhost:8080` Proxy inspection
   - `http://localhost:8080/version` Edge CDN Cache version
   - `http://localhost:8080/health` Proxy health status
   - `http://localhost:8080/statss` Proxy statistics
   - `http://localhost:8080/metrics` Proxy metrics compatible with Prometheus
   - `http://localhost:8080/cache` Proxy cache contents (very basic, but functional)

### Stop server

```bash
./stop.sh
```

---

## Testing Individual Requests

Individual requests can be sent to the proxy server using the `curl` command.

### Secure

`curl` to `https://localhost:6143/`

```bash
curl -s -o /dev/null -D - -k https://localhost:6143/ -H 'Host: github.com'
```

### Insecure

`curl` to `http://localhost:6188/`

```bash
curl -s -o /dev/null -D - -k http://localhost:6188/ -H 'Host: example.org'
```

### Subresources

In either case, to request a subresource belonging to the host, append the pathname to the proxy hostname.
E.G.:

```bash
curl -s -o /dev/null -D - -k https://localhost:6143/help/example-domains -H 'Host: www.iana.org'
```

### Useful `curl` Arguments

We're only interested in seeing the headers in the console, hence the arguments to keep `curl` silent (`-s`), drop the body output into a black hole (`-o /dev/null`) and direct the headers to stdout (`-D -`)

If you're using a self-signed server certificate, then the `curl` command must include the `-k` option in order to skip certificate validation.

Set the HTTP request `Host` header to the the name of the server you wish to access:  E.G. `-H 'Host: github.com'`

---

## Testing a Stream of Requests

One way to test a stream of requests is as follows:

1. Open the developer tools in your browser
2. Select the network tab, ensure that the log is empty and that you are recording network requests
3. Visit a web page that does not require authentication (such as <https://en.wikipedia.org/wiki/Main_Page>)
4. Stop recording the network log
5. Export the network log as a `.har` file to some local directory
6. Change into this repo's `./tests` directory
7. Run `./har2curl.mjs` passing the path to your `.har` file as the argument
8. The URLs recorded in the `.har` file will be requested via a `curl` command through the proxy and the headers printed to the console.

## Testing Cache Eviction

To test cache eviction, start the proxy with with a lower cache size, run several `.har` files through the cache, then look at the metrics page <http://localhost:8080/metrics> to see the number of evictions and the bytes evicted.

For example, to set a cache size of 1Mb:

```bash
CACHE_SIZE_BYTES=$((1024 * 1024)); cargo run
```

---

## Configuration

The CDN Edge Proxy can be configured using the following environment variables:

| Environment Variable | Default Value                  | Description                                   |
|----------------------|--------------------------------|-----------------------------------------------|
| `EDGE_RUNTIME_DIR`   | `/tmp/egde-sdn-store`          | Edge CDN Store runtime directory              |
| `CACHE_SIZE_BYTES`   | `2 * 1024 * 1024 * 1024` (2Gb) | Cache size in bytes                           |
| `PROXY_HTTP_PORT`    | `6143`                         | Port for HTTP connections                     |
| `PROXY_HTTPS_PORT`   | `6188`                         | Port for HTTPS connections                    |

---

## Seeing Debug Trace Output

To monitor the internal functionality of this cache server, configure the environment variable `RUST_LOG` to output information only for the relevant module:

```bash
RUST_LOG=edge_cdn_store=debug; cargo run
```

---

## Possible Port Clash With JetBrains IDEs

All JetBrains IDEs (including RustRover) start a process called `cef_server` that is part of the Chromium Embedded Framework (CEF) used for rendering web-based UI components.
Unfortunately, this service binds to port 6188, which is also the default HTTPS port used by the Pingora Framework.

This means that if you have RustRover running at the same time as you start the cache server, the server might be unable to bind port 6188.
If the Pingora Proxy is unsuccessful after a certain number of retries, it panics and gives up.

## Development Notes on Running this Proxy as a Daemon

[According to the documentation](https://github.com/cloudflare/pingora/blob/main/docs/user_guide/daemon.md), if you set `daemon: true` in `edge-cdn-store.yaml`, then start the server with the argument `--conf ./edge-cdn-store.yaml`, the Pingora framework will transform the server process into a daemon by forking it.
This happens when `server.run_forever()` is encountered.

This has several important consequences for the software architecture:
* Only the process that calls `fork()` survives.
* Any other processes or Tokio runtimes created prior to the fork are shut down. 
   This means the cache inspector must run as a Pingora background service rather than a `warp` server running in its own runtime.
* Any file descriptors opened before the fork are closed and are therefore no longer available to the server.
   This includes any file descriptors for `stdout` and `stderr`.
   Consequently, the logger must direct all its output to a file that is opened after the fork happens.
* After the fork, if the logger tries to write to `stdout` and `stderr`, the server crashes silently.
* Trapping panics must be done by defining an explicit panic handler in `std::panic::set_hook()`
* Trapping errors is trickier as not all runtime errors can be caught with `std::panic::catch_unwind()`

Unfortunately, once running as a daemon (on macOS at least), the software became very fragile and would crash often silently.
For example, in `pingora_proxy::ProxyHttp::upstream_peer()`, the call to `UpstreamPeer::new()` would crash the server.

I was further disappointed to discover that the `daemonize` crate in the Pingora framework has been marked as unmaintained: <https://github.com/cloudflare/pingora/issues/699>

Consequently, I have abandoned the attempt to get this server to run as a daemon.
It runs perfectly well as a background service.
