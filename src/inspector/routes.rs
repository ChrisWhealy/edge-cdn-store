use std::convert::Infallible;
use crate::{
    disk_cache::DiskCache,
    inspector::{
        display_disk_cache::handle_req, CACHE_CONTENTS_PATH, HEALTH_PATH, METRICS_PATH, STATS_PATH, VERSION_PATH,
    },
};

use prometheus::{Encoder, TextEncoder};
use serde::Serialize;
use std::sync::Arc;
use warp::{http::header, Filter, Reply};
use crate::disk_cache::cache_statistics::CacheStatistics;
use crate::disk_cache::eviction_manager_cfg;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Build inspector routes into a single Warp filter tree
pub fn build_inspector_routes(
    cache: Arc<&'static DiskCache>,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    let cache_clone1 = cache.clone();
    let cache_clone2 = cache.clone();
    let static_cache_ref = warp::any().map(move || cache.clone());

    let index = warp::path::end().and(warp::get()).map(|| {
        warp::reply::html(format!(
            r#"<html>
                     <head><title>Edge CDN Disk Cache Inspector</title></head>
                     <body>
                       <h1>Edge CDN Disk Cache Inspector</h1>
                       <ul>
                         <li><a href="/{VERSION_PATH}">Verion</a></li>
                         <li><a href="/{HEALTH_PATH}">Health</a></li>
                         <li><a href="/{STATS_PATH}">Statistics</a></li>
                         <li><a href="/{METRICS_PATH}">Metrics</a></li>
                         <li><a href="/{CACHE_CONTENTS_PATH}">Contents</a></li>
                       </ul>
                     </body>
                   </html>"#
        ))
    });

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // GET /version
    let show_version = warp::path(VERSION_PATH)
        .and(warp::get())
        .map(|| warp::reply::json(&serde_json::json!({ "version": env!("CARGO_PKG_VERSION") })));

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // GET /health
    #[derive(Serialize)]
    struct Health {
        status: &'static str,
    }
    let show_health = warp::path(HEALTH_PATH)
        .and(warp::get())
        .map(|| warp::reply::json(&Health { status: "ok" }));

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // GET /stats
    let show_stats = warp::path(STATS_PATH)
        .and(warp::get())
        .and(warp::any().map(move || cache_clone1.clone()))
        .and_then(|cache: Arc<&'static DiskCache>| async move {
            let cs = CacheStatistics {
                root: cache.root.clone(),
                start_time: cache.start_time,
                uptime: cache.start_time.elapsed().unwrap(),
                size_bytes_current: cache.metrics.size_bytes.get() as u64,
                size_bytes_max: eviction_manager_cfg().max_bytes as u64,
            };
            Ok::<_, Infallible>(warp::reply::json(&cs))
        });

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // GET /metrics
    let show_metrics =
        warp::path(METRICS_PATH)
            .and(warp::get())
            .and(static_cache_ref.clone())
            .map(|_cache: Arc<&DiskCache>| {
                let encoder = TextEncoder::new();
                let metric_families = prometheus::gather();
                let mut buffer = Vec::new();
                encoder.encode(&metric_families, &mut buffer).unwrap();
                warp::reply::with_header(buffer, header::CONTENT_TYPE, encoder.format_type()).into_response()
            });

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // GET /cache
    let cache_root = Arc::new(cache_clone2.root.clone());
    let show_cache = warp::path(CACHE_CONTENTS_PATH)
        .and(warp::path::tail()) // captures "" or "sub/dir/file"
        .and(warp::any().map({
            let root = cache_root.clone();
            move || root.clone()
        }))
        .and(warp::get().or(warp::head()).unify())
        .and_then(handle_req);

    index
        .or(show_version)
        .or(show_health)
        .or(show_stats)
        .or(show_metrics)
        .or(show_cache)
        .with(warp::trace::request())
}
