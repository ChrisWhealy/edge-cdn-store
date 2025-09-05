# PoC Edge CDN Cache

A CDN Edge reverse proxy PoC based on Cloudflare's [Pingora Framework](https://github.com/cloudflare/pingora).

The proposal is described [here](./docs/proposal.md)

## Generate TLS Certificate

This PoC requires your server certificate and private key are located in the `./keys` directory as `server.crt` and `server.pem`.

## Usage

```bash
cargo run
```

This starts three endpoints:

* `http://localhost:6188` insecure proxy endpoint  
* `https://localhost:6143` TLS proxy endpoint  
* `http://localhost:8080` proxy inspection  
   - `http://localhost:8080/metrics` proxy metrics compatible with Prometheus  
   - `http://localhost:8080/cache` proxy cache contents (very basic, but functional)

You will need to issue a `curl` command to the appropriate endpoint depending on whether you're accessing a secure or insecure address.

All cached responses are stored in the directory `.cache` immediately under `$CARGO_MANIFEST_DIR`.

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

### Useful `curl` Arguments

We're only interested in seeing the headers in the console, hence the arguments to keep `curl` silent (`-s`), drop the body output into a black hole (`-o /dev/null`) and direct the headers to stdout (`-D -`)

If you're using a self-signed server certificate, then the `curl` command must include the `-k` option on order to skip certificate validation.

Set the HTTP request `Host` header to the the name of the server you wish to access `-H 'Host: github.com'`

---

## Configuration

The port numbers used by the Pingora Proxy can be set from these environment variables:

| Scheme | Environment Variable | Defaults to
|--------|----------------------|-------------
| HTTP   | `PROXY_HTTP_PORT`    | `6188`
| HTTPS  | `PROXY_HTTPS_PORT`   | `6143`

If either of these environment variables are missing, or contain a value that cannot be parsed as a `u16`, then the default values will be used instead

---

## Seeing Debug Trace Output

To monitor the internal functionality of this cache server, set `RUST_LOG` to output information only for the relevant module:

```bash
RUST_LOG=edge_cdn_store=debug; cargo run
```

---

## Possible Port Clash With JetBrains IDEs

All JetBrains IDEs (including RustRover) start a process called `cef_server` that is part of the Chromium Embedded Framework (CEF) used for rendering web-based UI components.
Unfortunately, this service binds to port 6188, which is also the default HTTPS port used by the Pingora Framework.

This means that if you have RustRover running at the same time as you start the cache server, the server might be unable to bind port 6188.
If the Pingora Proxy is unsuccessful after a certain number of retries, it panics and gives up.
