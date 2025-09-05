use crate::disk_cache::DiskCache;

use mime_guess;
use pingora_cache::{
    key::{CacheHashKey, CompactCacheKey},
    CacheKey,
};
use pingora_error::{Error, ErrorType};
use prometheus::{Encoder, TextEncoder};
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use tokio::{fs, io};
use warp::{http::header, reply::Response as WarpResponse, Filter, Rejection, Reply};

static HEX_CHARS: &[u8] = b"0123456789ABCDEF";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Development helper functions
pub fn trace_fn_exit(fn_name: &str, err_msg: &str, trace_fn_enter: bool) {
    if trace_fn_enter {
        tracing::debug!("---> {fn_name}");
    }
    tracing::debug!("     {err_msg}");
    tracing::debug!("<--- {fn_name}");
}

pub fn trace_fn_exit_with_err<E>(fn_name: &str, err_msg: &str, trace_fn_enter: bool) -> pingora_error::Result<E> {
    trace_fn_exit(fn_name, err_msg, trace_fn_enter);
    Error::e_explain(ErrorType::InternalError, err_msg.to_string())
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn display_opt_str(opt_str: &Option<&str>) -> String {
    match opt_str {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "\"\"".to_string(),
    }
}

pub fn hex2str(hex: &[u8; 16]) -> String {
    let mut s = String::with_capacity(hex.len() * 2);

    for &byte in hex {
        s.push(HEX_CHARS[(byte >> 4) as usize] as char);
        s.push(HEX_CHARS[(byte & 0xf) as usize] as char);
    }
    s
}

#[allow(dead_code)]
pub fn format_cache_key(ck: &CacheKey) -> String {
    let mut buff: String = String::new();

    let ns = format!("Namespace: {}", display_opt_str(&ck.namespace_str()));
    let pk = format!("Primary Key: {}", display_opt_str(&ck.primary_key_str()));
    let p = format!("Primary: {:?}", hex2str(&ck.primary_bin()));
    let v = format!("Variance: {:?}", ck.variance());
    let ut = format!("User tag: {}", if ck.user_tag.is_empty() { "\"\"" } else { &ck.user_tag });
    let ext = format!("Extension: {:?}", ck.extensions);

    write!(buff, "{ns}, {pk}, {p}, {v}, {ut}, {ext}").unwrap();
    buff
}

#[allow(dead_code)]
pub fn format_compact_cache_key(ck: &CompactCacheKey) -> String {
    let mut buff: String = String::new();

    let p = format!("Primary: {:?}", hex2str(&ck.primary_bin()));
    let v = format!("Variance: {:?}", ck.variance());
    let ut = format!("User tag: {}", if ck.user_tag.is_empty() { "\"\"" } else { &ck.user_tag });

    write!(buff, "{p}, {v}, {ut}").unwrap();
    buff
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn start_disk_cache_inspector(cache: &'static DiskCache) {
    thread::spawn(move || {
        // Inject the static cache reference into handlers
        let with_cache = warp::any().map(move || cache);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for cache inspector");

        rt.block_on(async move {
            // /cache[/{tail...}] -> directory listing or file reply
            let cache_root = Arc::new(cache.root.clone());
            let show_cache = warp::path("cache")
                .and(warp::path::tail()) // captures "" or "sub/dir/file"
                .and(warp::any().map({
                    let root = cache_root.clone();
                    move || root.clone()
                }))
                .and(warp::get().or(warp::head()).unify())
                .and_then(handle_req);

            // /metrics -> Prometheus text format
            let metrics_route = warp::path!("metrics")
                .and(warp::get().or(warp::head()).unify())
                .and(with_cache)
                .map(|_cache: &'static DiskCache| {
                    let encoder = TextEncoder::new();
                    let metric_families = prometheus::gather();
                    let mut buffer = Vec::new();
                    encoder.encode(&metric_families, &mut buffer).unwrap();
                    warp::reply::with_header(buffer, header::CONTENT_TYPE, encoder.format_type()).into_response()
                });

            let routes = show_cache.or(metrics_route);

            tracing::info!("Cache inspector listening on http://127.0.0.1:8080");
            warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
        });
    });
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
