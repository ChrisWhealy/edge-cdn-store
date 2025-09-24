# PoC Edge CDN Cache

A CDN Edge reverse proxy PoC based on Cloudflare's [Pingora Framework](https://github.com/cloudflare/pingora).

The proposal is described [here](docs/README.md)

## Generate TLS Certificate

This PoC requires that your server certificate and private key files are located in the `./keys` directory as `server.crt` and `server.pem`.

## Usage

By default the server uses a runtime directory of `RUNTIME_DIR=/tmp/edge-cdn-store`

All cached responses are stored in the directory `cache` immediately under `$RUNTIME_DIR`.

### Start server

```bash
./start.sh edge-cdn-store.yaml
```

This starts three endpoints:

* `http://localhost:6188` insecure proxy endpoint
* `https://localhost:6143` TLS proxy endpoint
* `http://localhost:8080` Proxy inspection
   - `http://localhost:8080/version` Edge CDN Cache version
   - `http://localhost:8080/health` Proxy health status
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

| Environment Variable | Default Value                  | Description                                                                  |
|----------------------|--------------------------------|------------------------------------------------------------------------------|
| `CACHE_DIR`          | `./.cache`                     | The root directory for the disk cache<br>(relative to `$CARGO_MANIFEST_DIR`) |
| `CACHE_SIZE_BYTES`   | `2 * 1024 * 1024 * 1024` (2Gb) | Default cache size                                                           |
| `PROXY_HTTP_PORT`    | `6143`                         | Default port for HTTP connections                                            |
| `PROXY_HTTPS_PORT`   | `6188`                         | Default port for HTTPS connections                                           |

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
