mod consts;
mod disk_cache;
mod metrics;
mod proxy;
mod statics;
mod tiered;
mod utils;

use crate::{
    disk_cache::{disk_cache, eviction_manager_cfg, inspector::start_disk_cache_inspector},
    proxy::EdgeCdnProxy,
    statics::*,
    utils::env_var_or_num,
};

use crate::disk_cache::cache_statistics::PersistCacheOnShutdown;
use pingora::{prelude::*, server::RunArgs};
use std::error::Error;
use std::fs::OpenOptions;
use std::sync::Arc;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::EnvFilter;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn main() -> Result<(), Box<dyn Error>> {
    // Logging needs to be fork-safe
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path_to_app_log())
        .expect("open app.log");
    let (nb, _guard): (NonBlocking, _) = tracing_appender::non_blocking(log_file);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(nb)
        .init();

    // Trap panic output whilst daemonized
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path_to_panic_log()) {
            let _ = writeln!(f, "PANIC: {info}");
            let _ = writeln!(f, "Backtrace: {:?}", std::backtrace::Backtrace::capture());
        }
    }));

    let proxy_http_port: u16 = env_var_or_num("PROXY_HTTP_PORT", DEFAULT_PROXY_PORT_HTTP);
    let proxy_https_port: u16 = env_var_or_num("PROXY_HTTPS_PORT", DEFAULT_PROXY_PORT_HTTPS);

    let cert_path = format!("{}/server.crt", server_keys_dir());
    let key_path = format!("{}/server.pem", server_keys_dir());

    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy = EdgeCdnProxy::new(proxy_http_port, proxy_https_port);
    let mut service = http_proxy_service(&server.configuration, proxy);

    service.add_tcp(&format!("{IN_ADDR_ANY}:{proxy_http_port}"));
    service
        .add_tls(&format!("{IN_ADDR_ANY}:{proxy_https_port}"), &cert_path, &key_path)
        .unwrap();
    server.add_service(service);

    let persist_cache_svc = background_service(
        "persist cache on shutdown",
        PersistCacheOnShutdown { cache: Arc::new(disk_cache()) },
    );
    server.add_service(persist_cache_svc);

    tracing::info!(
        "Pingora proxies starting with cache size {} bytes",
        eviction_manager_cfg().max_bytes
    );
    tracing::info!("    HTTP  proxy listening on {IN_ADDR_ANY}:{}...", proxy_http_port);
    tracing::info!("    HTTPS proxy listening on {IN_ADDR_ANY}:{}...", proxy_https_port);

    start_disk_cache_inspector(disk_cache());

    // Run until a SIGINT/SIGTERM/SIGQUIT is received
    server.run_forever();
}
