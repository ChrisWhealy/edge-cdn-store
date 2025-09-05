use crate::{disk_cache::inspector::trace_fn_exit, DISK_CACHE};

use async_trait::async_trait;
use pingora::{
    http::ResponseHeader,
    prelude::{ProxyHttp, Session},
};
use pingora_cache::{storage::HandleHit, CacheKey, CacheMeta, ForcedInvalidationKind, NoCacheReason, RespCacheable};
use pingora_core::prelude::HttpPeer;
use pingora_error::{Error, ErrorType};
use std::time::{Duration, SystemTime};

const ONE_HOUR: Duration = Duration::from_secs(3600);
const HTTPS: &str = "https";
const HTTP: &str = "http";
const LOOPBACK_IP: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Parse a Host header (authority): host[:port] or [IPv6]:port
// Handle malformed values gracefully by returning HTTP 400 Bad Request
fn parse_host_authority(raw: &str) -> pingora_error::Result<(String, Option<u16>)> {
    // Handle malformed value - trailing slash
    let s = raw.trim().trim_end_matches('/');

    if s.is_empty() {
        Error::e_explain(ErrorType::HTTPStatus(400), "Host header empty")
    } else {
        // IPv6 address?
        if let Some(rest) = s.strip_prefix('[') {
            // IPv6 literal: "[::1]" or "[::1]:6143"
            if let Some(end) = rest.find(']') {
                let host = &rest[..end]; // no brackets for SNI
                let after = &rest[end + 1..]; // maybe ":port"
                let port = after.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
                Ok((host.to_string(), port))
            } else {
                Error::e_explain(ErrorType::HTTPStatus(400), "Malformed IPv6 address in Host")
            }
        } else {
            // hostname / IPv4: "example.com[:port]"
            if s.contains('/') || s.contains(' ') {
                Error::e_explain(ErrorType::HTTPStatus(400), "Invalid characters in Host")
            } else {
                let mut it = s.splitn(2, ':');
                let host = it.next().unwrap();
                let port = it.next().and_then(|p| p.parse::<u16>().ok());

                Ok((host.to_string(), port))
            }
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct MyProxy {
    self_addresses: Vec<String>,
    listen_http: &'static [u16],
    listen_https: &'static [u16],
}

impl MyProxy {
    pub fn new(listen_http: &'static [u16], listen_https: &'static [u16]) -> Self {
        let mut addresses = Vec::new();

        for port in listen_https.iter() {
            addresses.push(format!("{}:{}", LOOPBACK_IP, port));
            addresses.push(format!("{}:{}", LOCALHOST, port));
        }

        for port in listen_http.iter() {
            addresses.push(format!("{}:{}", LOOPBACK_IP, port));
            addresses.push(format!("{}:{}", LOCALHOST, port));
        }

        Self {
            self_addresses: addresses,
            listen_http,
            listen_https,
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = ();

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn new_ctx(&self) -> Self::CTX {
        ()
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> pingora_error::Result<Box<HttpPeer>> {
        let fn_name = "MyProxy::upstream_peer()";
        tracing::debug!("---> {fn_name}");

        // Retrieve Host header
        let host_hdr = match session.req_header().headers.get("Host").and_then(|h| h.to_str().ok()) {
            Some(h) => h,
            None => {
                return pingora_error::Error::e_explain(ErrorType::ConnectError, "Host header missing from request");
            },
        };

        let (host_only, port_from_host) = parse_host_authority(host_hdr)?;

        // Try to get scheme using the following fallback sequence:
        // --> ":scheme" pseudo header
        // --> "X-Forwarded-Proto"
        // --> default to http
        let hdr_scheme = session
            .req_header()
            .headers
            .get(":scheme")
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                session
                    .req_header()
                    .headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
            });
        let listener_https = session
            .server_addr()
            .and_then(|sa| sa.as_inet().map(|inet| inet.port()))
            .map(|p| self.listen_https.contains(&p))
            .unwrap_or(false);

        let use_https =
            hdr_scheme.map(|s| s.eq_ignore_ascii_case(HTTPS)).unwrap_or(listener_https) || port_from_host == Some(443);
        let port = port_from_host.unwrap_or(if use_https { 443 } else { 80 });
        let sni = if use_https { host_only.clone() } else { String::new() };

        tracing::debug!(
            "     origin: {}:{} tls={} sni={}",
            host_only,
            port,
            use_https,
            if use_https { &sni } else { "\"\"" }
        );

        let peer = HttpPeer::new((host_only, port), use_https, sni);
        tracing::debug!("<--- {fn_name}");
        Ok(Box::new(peer))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn request_cache_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> pingora_error::Result<()> {
        let fn_name = "MyProxy::request_cache_filter()";
        tracing::debug!("---> {fn_name}");

        let host = session
            .req_header()
            .headers
            .get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();

        // Cache must remain disabled for self-referencing requests
        if !self.self_addresses.iter().any(|addr| *addr == host) {
            let storage: &'static (dyn pingora_cache::storage::Storage + Sync) = *DISK_CACHE;
            session.cache.enable(storage, None, None, None, None);
            tracing::debug!("     Disk cache enabled");
        }

        tracing::debug!("<--- {fn_name}");
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // Build cache key from scheme+host+path
    fn cache_key_callback(&self, session: &Session, _ctx: &mut Self::CTX) -> pingora_error::Result<CacheKey> {
        let fn_name = "MyProxy::cache_key_callback()";
        tracing::debug!("---> {fn_name}");

        let host_hdr = session
            .req_header()
            .headers
            .get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        let (host_only, _port_opt) = match parse_host_authority(host_hdr) {
            Ok((host_only, port_opt)) => (host_only, port_opt),
            Err(parse_err) => {
                tracing::debug!("    {parse_err}");
                tracing::debug!("<--- {fn_name}");
                return Err(parse_err)
            }
        };

        let host_lc = host_only.to_ascii_lowercase();

        let hdr_scheme = session
            .req_header()
            .headers
            .get(":scheme")
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                session
                    .req_header()
                    .headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
            });
        let use_https = session
            .server_addr()
            .and_then(|sa| sa.as_inet().map(|inet| inet.port()))
            .map(|p| self.listen_https.contains(&p))
            .unwrap_or(false);
        let scheme = hdr_scheme
            .map(|s| if s.eq_ignore_ascii_case(HTTPS) { HTTPS } else { HTTP })
            .unwrap_or(if use_https { HTTPS } else { HTTP });
        let path_q = session.req_header().uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        let primary = format!("{scheme}://{host_lc}{path_q}");

        tracing::debug!("     cache key primary = {primary}");
        tracing::debug!("<--- {fn_name}");

        Ok(CacheKey::new(&[], primary.as_bytes(), ""))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn cache_hit_filter(
        &self,
        _session: &mut Session,
        _meta: &CacheMeta,
        _hit: &mut Box<dyn HandleHit + Send + Sync>,
        _is_fresh: bool,
        _ctx: &mut Self::CTX,
    ) -> pingora_error::Result<Option<ForcedInvalidationKind>> {
        // If needed, forced invalidation could happen here
        Ok(None)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn response_cache_filter(
        &self,
        _session: &Session,
        resp: &ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_error::Result<RespCacheable> {
        let fn_name = "MyProxy::response_cache_filter()";
        let fn_exit = format!("<--- {fn_name}");
        tracing::debug!("---> {fn_name}");
        let status = resp.as_ref().status;

        // Non-2xx status codes are not cached
        if !status.is_success() {
            trace_fn_exit(fn_name, &format!("Not caching response: HTTP {status}"), false);
            return Ok(RespCacheable::Uncacheable(NoCacheReason::Custom("non-2xx response")));
        }

        // Respect Cache-Control: no-store
        if let Some(cc) = resp.as_ref().headers.get("cache-control").and_then(|v| v.to_str().ok()) {
            if cc.to_ascii_lowercase().contains("no-store") {
                trace_fn_exit(fn_name, "Caching forbidden due to Cache-Control: no-store", false);
                return Ok(RespCacheable::Uncacheable(NoCacheReason::OriginNotCache));
            }
        }

        // Otherwise, make it cacheable for 1 hour
        let now = SystemTime::now();
        let meta = CacheMeta::new(now + ONE_HOUR, now, 0, 0, resp.clone());
        let response = RespCacheable::Cacheable(meta);

        tracing::debug!("{fn_exit}");
        Ok(response)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_resp: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_error::Result<()> {
        upstream_resp.insert_header("x-cdn-cache", "MISS").ok();
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn response_filter(
        &self,
        session: &mut Session,
        resp: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_error::Result<()> {
        let state = if session.cache.upstream_used() {
            "MISS" // fetched from origin
        } else {
            "HIT" // fetched from cache
        };

        resp.insert_header("x-cdn-cache", state).ok();
        Ok(())
    }
}
