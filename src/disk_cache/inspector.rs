use crate::disk_cache::DiskCache;

use async_trait::async_trait;
use mime_guess;
use pingora_core::{server::ShutdownWatch, services::background::BackgroundService};
use prometheus::{Encoder, TextEncoder};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use tokio::{
    fs, io,
    sync::{oneshot, Mutex},
};
use warp::{http::header, reply::Response as WarpResponse, Filter, Rejection, Reply};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
const VERSION_PATH: &'static str = "version";
const HEALTH_PATH: &'static str = "health";
const METRICS_PATH: &'static str = "metrics";
const CACHE_CONTENTS_PATH: &'static str = "cache";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct InspectorHandle {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl InspectorHandle {
    pub fn new(tx: oneshot::Sender<()>, thread: thread::JoinHandle<()>) -> Self {
        Self {
            shutdown_tx: Mutex::new(Some(tx)),
            thread: Mutex::new(Some(thread)),
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct StopInspectorOnShutdown {
    pub inspector: Arc<InspectorHandle>,
}

#[async_trait]
impl BackgroundService for StopInspectorOnShutdown {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        // Wait for graceful shutdown
        // This is only fired for SIGTERM or SIGQUIT, NOT SIGINT
        let _ = shutdown.changed().await;

        // Signal warp to stop
        if let Some(tx) = self.inspector.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
            tracing::info!("inspector: sent shutdown");
        } else {
            tracing::warn!("inspector: shutdown signal already sent or not initialized");
        }

        // Joining a std::thread is blocking; do it off the reactor:
        if let Some(th) = self.inspector.thread.lock().await.take() {
            tokio::task::spawn_blocking(move || {
                let _ = th.join();
                tracing::info!("inspector: thread joined");
            });
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn start_disk_cache_inspector(addr: std::net::SocketAddr, cache: Arc<&'static DiskCache>) -> Arc<InspectorHandle> {
    let (tx, rx) = oneshot::channel::<()>();
    let routes = build_inspector_routes(cache);

    let th = std::thread::Builder::new()
        .name("disk-cache-inspector".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("inspector: build tokio rt");

            rt.block_on(async move {
                warp::serve(routes)
                    .bind(addr)
                    .await
                    .graceful(async move {
                        let _ = rx.await;
                    })
                    .run()
                    .await;
            });
        })
        .expect("inspector: spawn thread");

    Arc::new(InspectorHandle::new(tx, th))
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Build inspector routes into a single Warp filter tree
pub fn build_inspector_routes(
    cache: Arc<&'static DiskCache>,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    let cache_clone = cache.clone();
    let static_cache_ref = warp::any().map(move || cache.clone());

    let index = warp::path::end().and(warp::get()).map(|| {
        warp::reply::html(format!(
            r#"<html>
                     <head><title>Edge CDN Disk Cache Inspector</title></head>
                     <body>
                       <h1>Edge CDN Disk Cache Inspector</h1>
                       <ul>
                         <li><a href="/{VERSION_PATH}">/{VERSION_PATH}</a></li>
                         <li><a href="/{HEALTH_PATH}">/{HEALTH_PATH}</a></li>
                         <li><a href="/{METRICS_PATH}">/{METRICS_PATH}</a></li>
                         <li><a href="/{CACHE_CONTENTS_PATH}">/{CACHE_CONTENTS_PATH}</a></li>
                       </ul>
                     </body>
                   </html>"#
        ))
    });

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
    // GET /cache
    let cache_root = Arc::new(cache_clone.root.clone());
    let show_cache = warp::path(CACHE_CONTENTS_PATH)
        .and(warp::path::tail()) // captures "" or "sub/dir/file"
        .and(warp::any().map({
            let root = cache_root.clone();
            move || root.clone()
        }))
        .and(warp::get().or(warp::head()).unify())
        .and_then(handle_req);

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
    // GET /version
    let show_version = warp::path(VERSION_PATH)
        .and(warp::get())
        .map(|| warp::reply::json(&serde_json::json!({ "version": env!("CARGO_PKG_VERSION") })));

    index
        .or(show_version)
        .or(show_health)
        .or(show_cache)
        .or(show_metrics)
        // .or(flush)
        .with(warp::trace::request())
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
async fn handle_req(tail: warp::path::Tail, root: Arc<PathBuf>) -> Result<WarpResponse, Rejection> {
    let tail_str = tail.as_str();
    let path = resolve_under_root(&root, tail_str)
        .await
        .map_err(|_| warp::reject::not_found())?;

    // directory => return HTML as Response<Body>
    let reply = if fs::metadata(&path).await.map(|m| m.is_dir()).unwrap_or(false) {
        let html = render_dir_listing(&path, tail_str)
            .await
            .map_err(|_| warp::reject::not_found())?;
        warp::reply::html(html).into_response()
    } else {
        // file => stream it as Body
        let data = fs::read(&path).await.map_err(|_| warp::reject::not_found())?;
        let mime = mime_guess::from_path(&path).first_or_octet_stream();

        warp::reply::with_header(data, header::CONTENT_TYPE, mime.as_ref()).into_response()
    };

    Ok(reply)
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
async fn resolve_under_root(root: &Path, tail: &str) -> io::Result<PathBuf> {
    let joined = root.join(tail);
    let root_canon = root.canonicalize()?;

    // Prevent path escaping out of root
    let full = match fs::metadata(&joined).await {
        Ok(_) => joined.canonicalize()?,
        Err(_) => joined.clone(),
    };

    if full.starts_with(&root_canon) {
        Ok(full)
    } else {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "path escapes outside root"))
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
async fn render_dir_listing(path: &Path, tail: &str) -> io::Result<String> {
    let mut entries = fs::read_dir(path).await?;
    let mut items = Vec::<(String, bool)>::new(); // (name, is_dir)

    while let Some(e) = entries.next_entry().await? {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
        items.push((name, is_dir));
    }

    // Sort: directories first, then files, case-insensitive by name.
    items.sort_by(|a, b| (b.1, a.0.to_lowercase()).cmp(&(a.1, b.0.to_lowercase())).reverse());

    // Normalise the tail we got from warp::path::tail()
    let tail_norm = tail.trim_start_matches('/').to_string();
    let base = if tail_norm.is_empty() {
        "/cache".to_string()
    } else {
        format!("/cache/{}", tail_norm.trim_end_matches('/'))
    };

    let mut html = String::new();
    html.push_str("<!doctype html><meta charset=utf-8>");
    html.push_str("<style>body{font:14px system-ui;margin:2rem} a{text-decoration:none} .dir{font-weight:600}</style>");
    html.push_str(&format!("<h1>Index of /{}</h1><ul>", html_escape(&tail_norm)));

    // Parent link (if not at root)
    if !tail_norm.is_empty() {
        let t = tail_norm.trim_end_matches('/');
        let parent_href = match t.rfind('/') {
            Some(idx) if idx > 0 => format!("/cache/{}/", &t[..idx]),
            _ => "/cache/".to_string(),
        };
        html.push_str(&format!("<li>📁 <a class='dir' href='{parent_href}'>../</a></li>"));
    }

    for (name, is_dir) in items {
        let enc = urlencoding::encode(&name); // encode only the name component
        let sep = if base.ends_with('/') { "" } else { "/" };
        let mut href = format!("{base}{sep}{enc}");

        if is_dir {
            // Make sure trailing slash is not encoded...
            href.push('/')
        };

        let display = if is_dir { format!("{name}/") } else { name.clone() };
        let icon = if is_dir { "📁" } else { "📄" };
        let class = if is_dir { "class='dir'" } else { "" };

        html.push_str(&format!(
            "<li>{icon} <a {class} href='{href}'>{}</a></li>",
            html_escape(&display)
        ));
    }

    html.push_str("</ul>");
    Ok(html)
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Encode characters that must not be interpreted as HTML
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
