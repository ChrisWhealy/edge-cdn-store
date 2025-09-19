mod disk_cache;
mod metrics;
mod proxy;
mod tiered;
mod utils;

use crate::{
    disk_cache::{cache_statistics::PersistCacheOnShutdown, inspector::start_disk_cache_inspector, DiskCache},
    proxy::EdgeCdnProxy,
    tiered::{TieredStorage, WritePolicy},
    utils::{env_var_or_num, env_var_or_str},
};

use once_cell::sync::Lazy;
use pingora::prelude::*;
use pingora_cache::eviction::simple_lru::Manager as LruManager;
use std::{
    error::Error,
    fs::OpenOptions,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::EnvFilter;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
static DEFAULT_PROXY_PORT_HTTP: &'static [u16] = &[6188];
static DEFAULT_PROXY_PORT_HTTPS: &'static [u16] = &[6143];
static DEFAULT_CACHE_SIZE_BYTES: &'static usize = &(2 * 1024 * 1024 * 1024); // Default cache size = 2Gb
static IN_ADDR_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
static LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
pub static RUNTIME_DIR: &'static str = "/tmp/edge-cdn-store";
pub static CACHE_DIR: Lazy<String> = Lazy::new(|| format!("{RUNTIME_DIR}/cache"));
pub static KEY_DIR: Lazy<String> = Lazy::new(|| format!("{RUNTIME_DIR}/keys"));
pub static PATH_TO_APP_LOG: Lazy<String> = Lazy::new(|| format!("{RUNTIME_DIR}/app.log"));
pub static PATH_TO_PANIC_LOG: Lazy<String> = Lazy::new(|| format!("{RUNTIME_DIR}/panic.log"));

pub static CACHE_STATE_FILENAME: &str = "_cache_state.json";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Define cache
static DISK_CACHE: Lazy<&'static DiskCache> =
    Lazy::new(|| Box::leak(Box::new(DiskCache::new(env_var_or_str("CACHE_DIR", &*CACHE_DIR)))));

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
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    // Logging needs to be fork-safe
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*PATH_TO_APP_LOG)
        .expect("open app.log");
    let (nb, _guard): (NonBlocking, _) = tracing_appender::non_blocking(log_file);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(nb)
        .init();

    // Trap panic output whilst daemonized
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&*PATH_TO_PANIC_LOG) {
            let _ = writeln!(f, "PANIC: {info}");
            let _ = writeln!(f, "Backtrace: {:?}", std::backtrace::Backtrace::capture());
        }
    }));

    let proxy_port_http: u16 = env_var_or_num("PROXY_PORT_HTTP", DEFAULT_PROXY_PORT_HTTP[0]);
    let proxy_port_https: u16 = env_var_or_num("PROXY_PORT_HTTPS", DEFAULT_PROXY_PORT_HTTPS[0]);

    let cert_path = format!("{}/server.crt", &*KEY_DIR);
    let key_path = format!("{}/server.pem", &*KEY_DIR);

    let server_args = Opt::parse_args();
    let mut server = Server::new(Some(server_args))?;
    server.bootstrap();

    let proxy = EdgeCdnProxy::new(proxy_port_http, proxy_port_https);
    let mut service = http_proxy_service(&server.configuration, proxy);

    service.add_tcp(&format!("{IN_ADDR_ANY}:{proxy_port_http}"));
    service.add_tcp(&format!("[::]:{proxy_port_http}"));
    service
        .add_tls(&format!("{IN_ADDR_ANY}:{proxy_port_https}"), &cert_path, &key_path)
        .unwrap();
    service.add_tls(&format!("[::]:{proxy_port_https}"), &cert_path, &key_path)?;

    server.add_service(service);

    let persist_cache_svc = background_service(
        "persist cache on shutdown",
        PersistCacheOnShutdown { cache: Arc::new(*DISK_CACHE) },
    );
    server.add_service(persist_cache_svc);

    tracing::info!("Pingora proxies starting with cache size {} bytes", EVICT_CFG.max_bytes);
    tracing::info!("    HTTP  proxy listening on {IN_ADDR_ANY}:{}...", proxy_port_http);
    tracing::info!("    HTTPS proxy listening on {IN_ADDR_ANY}:{}...", proxy_port_https);

    start_disk_cache_inspector(*DISK_CACHE);

    server.run_forever();
}
