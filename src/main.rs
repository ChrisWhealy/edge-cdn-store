mod consts;
mod disk_cache;
mod metrics;
mod proxy;
mod statics;
mod tiered;
mod utils;

use crate::{
    disk_cache::{
        cache_statistics::persist_cache_state, disk_cache, eviction_manager_cfg, inspector::start_disk_cache_inspector,
    },
    proxy::EdgeCdnProxy,
    statics::*,
    utils::env_var_or_num,
};

use pingora::{prelude::*, server::RunArgs};
use std::error::Error;
use tracing_subscriber::EnvFilter;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let proxy_http_port: u16 = env_var_or_num("PROXY_HTTP_PORT", DEFAULT_PROXY_PORT_HTTP);
    let proxy_https_port: u16 = env_var_or_num("PROXY_HTTPS_PORT", DEFAULT_PROXY_PORT_HTTPS);

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

    tracing::info!(
        "Pingora proxies starting with cache size {} bytes",
        eviction_manager_cfg().max_bytes
    );
    tracing::info!("    HTTP  proxy listening on {IN_ADDR_ANY}:{}...", proxy_http_port);
    tracing::info!("    HTTPS proxy listening on {IN_ADDR_ANY}:{}...", proxy_https_port);

    start_disk_cache_inspector(disk_cache());

    // Run until a SIGINT/SIGTERM/SIGQUIT is received
    server.run(RunArgs::default());

    tracing::info!("Pingora proxies shut down");
    disk_cache().set_uptime_now();
    let _ = persist_cache_state(disk_cache());

    Ok(())
}
