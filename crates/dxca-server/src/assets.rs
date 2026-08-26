//! Serves the embedded web UI (`web-ui/dist`, baked in at compile time by
//! `include_dir` — build.rs guarantees the dir exists). One binary, no
//! files-on-disk to deploy (plan §9).

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use include_dir::{Dir, include_dir};

static DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web-ui/dist");

/// Fallback handler: exact file match, else `index.html` (the Svelte app
/// owns client-side routing from M5 on).
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let file = if path.is_empty() {
        DIST.get_file("index.html")
    } else {
        DIST.get_file(path).or_else(|| DIST.get_file("index.html"))
    };
    match file {
        Some(f) => {
            let mime = mime_for(f.path().extension().and_then(|e| e.to_str()));
            ([(header::CONTENT_TYPE, mime)], f.contents()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Tiny by-extension table instead of a mime crate — the embedded dist only
/// ever contains what Vite emits.
fn mime_for(ext: Option<&str>) -> &'static str {
    match ext {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
