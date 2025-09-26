mod consts;
mod disk_cache;
mod inspector;
mod logger;
mod metrics;
mod proxy;
mod statics;
mod tiered;
mod utils;

use crate::{
    consts::{DEFAULT_PROXY_PORT_HTTP, DEFAULT_PROXY_PORT_HTTPS},
    disk_cache::{cache_statistics::PersistCacheOnShutdown, disk_cache, eviction_manager_cfg},
    inspector::{start_disk_cache_inspector, StopInspectorOnShutdown},
    logger::BackgroundLogger,
    proxy::EdgeCdnProxy,
    statics::*,
    utils::env_var_or_num,
};

use pingora::prelude::*;
use pingora_core::server::{configuration::Opt, Server};
use std::{error::Error, fs::OpenOptions, sync::Arc};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn main() -> Result<(), Box<dyn Error>> {
    // Ensure panic output is trapped
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;

        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path_to_panic_log()) {
            let _ = writeln!(f, "PANIC: {info}");
            let _ = writeln!(f, "Backtrace: {:?}", std::backtrace::Backtrace::force_capture());
        }
    }));

    // Create a Pingora server based on command line options
    let mut server = Server::new(Some(Opt::parse_args()))?;
    server.bootstrap();

    // Enable background logging
    let logger = background_service(
        "background logger",
        BackgroundLogger {
            path: path_to_app_log().into(),
            // filter: None,
            // Debug switched on for development purposes
            filter: Some("edge_cdn_store=debug,pingora=info".into()),
        },
    );
    server.add_service(logger);

    let proxy_http_port: u16 = env_var_or_num("PROXY_HTTP_PORT", DEFAULT_PROXY_PORT_HTTP);
    let proxy_https_port: u16 = env_var_or_num("PROXY_HTTPS_PORT", DEFAULT_PROXY_PORT_HTTPS);
    let mut service = http_proxy_service(&server.configuration, EdgeCdnProxy::new(proxy_http_port, proxy_https_port));

    service.add_tcp(&format!("{IN_ADDR_ANY}:{proxy_http_port}"));
    service.add_tls(
        &format!("{IN_ADDR_ANY}:{proxy_https_port}"),
        &format!("{}/server.crt", server_keys_dir()),
        &format!("{}/server.pem", server_keys_dir()),
    )?;
    server.add_service(service);

    let persist_cache_svc = background_service(
        "persist cache on shutdown",
        PersistCacheOnShutdown { cache: Arc::new(disk_cache()) },
    );
    server.add_service(persist_cache_svc);

    // Start inspector on port 8080
    let inspector = start_disk_cache_inspector((IN_ADDR_ANY, 8080).into(), Arc::new(disk_cache()));

    let stop_inspector_svc = background_service(
        "stop inspector on shutdown",
        StopInspectorOnShutdown { inspector: inspector.clone() },
    );
    server.add_service(stop_inspector_svc);

    tracing::info!(
        "Pingora proxies starting with cache size {} bytes",
        eviction_manager_cfg().max_bytes
    );
    tracing::info!("    HTTP  proxy listening on {IN_ADDR_ANY}:{}...", proxy_http_port);
    tracing::info!("    HTTPS proxy listening on {IN_ADDR_ANY}:{}...", proxy_https_port);

    // Run until a SIGINT/SIGTERM/SIGQUIT is received
    // Only SIGTERM and SIGQUIT will trigger a graceful shutdown
    server.run_forever();
}
