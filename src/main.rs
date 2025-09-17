mod disk_cache;
mod metrics;
mod proxy;
mod tiered;
mod utils;

use crate::{
    disk_cache::{cache_statistics::persist_cache_state, inspector::start_disk_cache_inspector, DiskCache},
    proxy::EdgeCdnProxy,
    tiered::{TieredStorage, WritePolicy},
    utils::{env_var_or_num, env_var_or_str},
};

use once_cell::sync::Lazy;
use pingora::{prelude::*, server::RunArgs};
use pingora_cache::eviction::simple_lru::Manager as LruManager;
use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr},
};
use tracing_subscriber::EnvFilter;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
static DEFAULT_PROXY_HTTP_PORT: &'static [u16] = &[6188];
static DEFAULT_PROXY_HTTPS_PORT: &'static [u16] = &[6143];
static DEFAULT_CACHE_SIZE_BYTES: &'static usize = &(2 * 1024 * 1024 * 1024); // Default cache size = 2Gb
static IN_ADDR_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
static DEFAULT_CACHE_DIR: &'static str = "./.cache";

pub static CACHE_STATE_FILENAME: &str = "_cache_state.json";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Define cache and eviction policy
static DISK_CACHE: Lazy<&'static DiskCache> =
    Lazy::new(|| Box::leak(Box::new(DiskCache::new(env_var_or_str("CACHE_DIR", DEFAULT_CACHE_DIR)))));

// TODO Implement some sort of remote cache
// static REMOTE: Lazy<&'static RemoteCache> = Lazy::new(|| Box::leak(Box::new(RemoteCache::new())));

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Remote cache to be wired in later
pub static TIERED: Lazy<&'static TieredStorage> = Lazy::new(|| {
    Box::leak(Box::new(TieredStorage::new(
        *DISK_CACHE,
        None,
        // Some(*REMOTE),         // Need to implement this
        WritePolicy::PrimaryOnly, // Switches to WriteThroughBoth when remote is available
    )))
});

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Define eviction policy - just using LRU at the moment
pub struct EvictCfg {
    pub max_bytes: usize,
}
pub static EVICT_CFG: Lazy<EvictCfg> = Lazy::new(|| EvictCfg {
    max_bytes: env_var_or_num("CACHE_SIZE_BYTES", *DEFAULT_CACHE_SIZE_BYTES),
});
pub static EVICT: Lazy<&'static LruManager> = Lazy::new(|| Box::leak(Box::new(LruManager::new(EVICT_CFG.max_bytes))));

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let proxy_http_port: u16 = env_var_or_num("PROXY_HTTP_PORT", DEFAULT_PROXY_HTTP_PORT[0]);
    let proxy_https_port: u16 = env_var_or_num("PROXY_HTTPS_PORT", DEFAULT_PROXY_HTTPS_PORT[0]);

    let cert_path = format!("{}/keys/server.crt", env!("CARGO_MANIFEST_DIR"));
    let key_path = format!("{}/keys/server.pem", env!("CARGO_MANIFEST_DIR"));

    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy = EdgeCdnProxy::new(proxy_http_port, proxy_https_port);
    let mut service = http_proxy_service(&server.configuration, proxy);

    service.add_tcp(&format!("{IN_ADDR_ANY}:{proxy_http_port}"));
    service
        .add_tls(&format!("{IN_ADDR_ANY}:{proxy_https_port}"), &cert_path, &key_path)
        .unwrap();
    server.add_service(service);

    tracing::info!("Pingora proxies starting with cache size {} bytes", EVICT_CFG.max_bytes);
    tracing::info!("    HTTP  proxy listening on {IN_ADDR_ANY}:{}...", proxy_http_port);
    tracing::info!("    HTTPS proxy listening on {IN_ADDR_ANY}:{}...", proxy_https_port);

    start_disk_cache_inspector(*DISK_CACHE);

    // Run until a SIGINT/SIGTERM/SIGQUIT is received
    server.run(RunArgs::default());

    tracing::info!("Pingora proxies shut down");
    DISK_CACHE.set_uptime_now();
    let _ = persist_cache_state(*DISK_CACHE);

    Ok(())
}
