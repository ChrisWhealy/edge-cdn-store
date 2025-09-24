use crate::{inspector::WarpResponse, utils::html_escape};

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{fs, io};
use warp::{http::header, Rejection, Reply};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub(crate) async fn handle_req(tail: warp::path::Tail, root: Arc<PathBuf>) -> Result<WarpResponse, Rejection> {
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
