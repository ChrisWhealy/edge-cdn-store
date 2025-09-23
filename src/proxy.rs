use crate::{
    consts::{HTTP, HTTPS, ONE_HOUR},
    disk_cache::eviction_manager,
    statics::LOCALHOST,
    tiered::tiered_cache,
    utils::{impl_trace, parse_host_authority, scheme_from_hdr, trace_fn_exit, trace_fn_exit_with_err, Trace},
};

use crate::statics::{DEFAULT_PORT_HTTP, DEFAULT_PORT_HTTPS};
use async_trait::async_trait;
use pingora::{
    http::ResponseHeader,
    prelude::{ProxyHttp, Session},
};
use pingora_cache::{storage::HandleHit, CacheKey, CacheMeta, ForcedInvalidationKind, NoCacheReason, RespCacheable};
use pingora_core::prelude::HttpPeer;
use pingora_error::{Error, ErrorType};
use std::time::SystemTime;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[allow(dead_code)]
pub struct EdgeCdnProxy {
    self_addresses: Vec<String>,
    listen_http: u16,
    listen_https: u16,
}

impl_trace!(EdgeCdnProxy);

impl EdgeCdnProxy {
    pub fn new(listen_http: u16, listen_https: u16) -> Self {
        <Self as Trace>::fn_enter_exit("new");
        let mut addresses = Vec::new();

        addresses.push(format!("{}:{}", LOCALHOST, listen_http));
        addresses.push(format!("{}:{}", LOCALHOST, listen_http));
        addresses.push(format!("{}:{}", LOCALHOST, listen_https));
        addresses.push(format!("{}:{}", LOCALHOST, listen_https));

        Self {
            self_addresses: addresses,
            listen_http,
            listen_https,
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl ProxyHttp for EdgeCdnProxy {
    type CTX = ();

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn new_ctx(&self) -> Self::CTX {
        ()
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> pingora_error::Result<Box<HttpPeer>> {
        let fn_name = "upstream_peer";
        <Self as Trace>::fn_enter(fn_name);

        // Retrieve Host header
        let host_hdr = match session.req_header().headers.get("Host").and_then(|h| h.to_str().ok()) {
            Some(h) => h,
            None => {
                <Self as Trace>::fn_exit(fn_name);
                return Error::e_explain(ErrorType::ConnectError, "Host header missing from request");
            },
        };

        let (host_only, port_from_host) = match parse_host_authority(host_hdr) {
            Ok(host_and_port) => host_and_port,
            Err(ha_err) => {
                trace_fn_exit(fn_name, &ha_err.to_string(), false);
                return Err(ha_err);
            },
        };

        let hdr_scheme = scheme_from_hdr(session);
        let listener_https = session
            .server_addr()
            .and_then(|sa| sa.as_inet().map(|inet| inet.port()))
            .map(|p| self.listen_https.eq(&p))
            .unwrap_or(false);
        let use_https = hdr_scheme.map(|s| s.eq_ignore_ascii_case(HTTPS)).unwrap_or(listener_https)
            || port_from_host == Some(DEFAULT_PORT_HTTPS);
        let port = port_from_host.unwrap_or(if use_https { DEFAULT_PORT_HTTPS } else { DEFAULT_PORT_HTTP });
        let sni = if use_https { host_only.clone() } else { String::new() };

        tracing::debug!(
            "     origin: {}:{} tls={} sni={}",
            host_only,
            port,
            use_https,
            if use_https { &sni } else { "\"\"" }
        );

        let peer = HttpPeer::new((host_only, port), use_https, sni);
        <Self as Trace>::fn_exit(fn_name);
        Ok(Box::new(peer))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn request_cache_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> pingora_error::Result<()> {
        let fn_name = "request_cache_filter";
        <Self as Trace>::fn_enter(fn_name);

        let host = session
            .req_header()
            .headers
            .get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();

        // Cache must remain disabled for self-referencing requests
        if !self.self_addresses.iter().any(|addr| *addr == host) {
            session.cache.enable(tiered_cache(), Some(eviction_manager()), None, None, None);
            tracing::debug!("     Disk cache enabled");
        }

        <Self as Trace>::fn_exit(fn_name);
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // Build cache key from scheme+host+path
    fn cache_key_callback(&self, session: &Session, _ctx: &mut Self::CTX) -> pingora_error::Result<CacheKey> {
        let fn_name = "cache_key_callback";
        <Self as Trace>::fn_enter(fn_name);

        let host_hdr = session
            .req_header()
            .headers
            .get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        let (host_only, _port_opt) = match parse_host_authority(host_hdr) {
            Ok((host_only, port_opt)) => (host_only, port_opt),
            Err(parse_err) => return trace_fn_exit_with_err(fn_name, &parse_err.to_string(), false),
        };
        let host_lc = host_only.to_ascii_lowercase();
        let hdr_scheme = scheme_from_hdr(session);
        let use_https = session
            .server_addr()
            .and_then(|sa| sa.as_inet().map(|inet| inet.port()))
            .map(|p| self.listen_https.eq(&p))
            .unwrap_or(false);
        let scheme = hdr_scheme
            .map(|s| if s.eq_ignore_ascii_case(HTTPS) { HTTPS } else { HTTP })
            .unwrap_or(if use_https { HTTPS } else { HTTP });
        let path_q = session.req_header().uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        let primary = format!("{scheme}://{host_lc}{path_q}");

        tracing::debug!("     cache key primary = {primary}");
        <Self as Trace>::fn_exit(fn_name);

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
        let fn_name = "response_cache_filter";
        <Self as Trace>::fn_enter(fn_name);
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

        <Self as Trace>::fn_exit(fn_name);
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
