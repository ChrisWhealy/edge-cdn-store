mod disk_cache;
mod metrics;
mod proxy;

use crate::{
    disk_cache::{inspector::start_disk_cache_inspector, DiskCache},
    proxy::MyProxy,
};

use once_cell::sync::Lazy;
use pingora::prelude::*;
use std::error::Error;
use tracing_subscriber::EnvFilter;

static DEFAULT_PROXY_HTTP_PORT: &'static [u16] = &[6188];
static DEFAULT_PROXY_HTTPS_PORT: &'static [u16] = &[6143];

// Leak a global static reference, required by `HttpCache::enable`
static DISK_CACHE: Lazy<&'static DiskCache> = Lazy::new(|| Box::leak(Box::new(DiskCache::new("./.cache"))));

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn env_var_or_u16(var_name: &str, u16_default: u16) -> u16 {
    if let Ok(val) = std::env::var(var_name) {
        val.trim().parse::<u16>().unwrap_or(u16_default)
    } else {
        u16_default
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let proxy_http_port = env_var_or_u16("PROXY_HTTP_PORT", DEFAULT_PROXY_HTTP_PORT[0]);
    let proxy_https_port = env_var_or_u16("PROXY_HTTPS_PORT", DEFAULT_PROXY_HTTPS_PORT[0]);

    let cert_path = format!("{}/keys/server.crt", env!("CARGO_MANIFEST_DIR"));
    let key_path = format!("{}/keys/server.pem", env!("CARGO_MANIFEST_DIR"));

    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy = MyProxy::new(DEFAULT_PROXY_HTTP_PORT, DEFAULT_PROXY_HTTPS_PORT);
    let mut service = http_proxy_service(&server.configuration, proxy);

    service.add_tcp(&format!("127.0.0.1:{proxy_http_port}"));
    service
        .add_tls(&format!("127.0.0.1:{proxy_https_port}"), &cert_path, &key_path)
        .unwrap();
    server.add_service(service);

    tracing::info!("Pingora proxies starting");
    tracing::info!("    HTTP proxy listening on 127.0.0.1:{}...", proxy_http_port);
    tracing::info!("    HTTPS proxy listening on 127.0.0.1:{}...", proxy_https_port);

    start_disk_cache_inspector(*DISK_CACHE);

    server.run_forever();
}
