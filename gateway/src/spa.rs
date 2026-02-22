//! Embedded SPA handler for the ShadowMesh dashboard.
//!
//! Uses `rust-embed` to compile the React build output (`dashboard/dist/`)
//! directly into the gateway binary. Serves static assets with appropriate
//! caching headers, and falls back to `index.html` for client-side routing.

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../dashboard/dist"]
struct DashboardAssets;

/// Serve embedded SPA assets.
///
/// 1. Try to match the URI path to an embedded file (e.g. `assets/index-abc.js`).
/// 2. If no file matches, serve `index.html` so React Router handles the route.
pub async fn spa_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact file match first
    if !path.is_empty() {
        if let Some(file) = DashboardAssets::get(path) {
            return serve_file(path, &file);
        }
    }

    // Fallback: serve index.html for SPA routing
    match DashboardAssets::get("index.html") {
        Some(file) => {
            let body = file.data.to_vec();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                body,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Dashboard not built").into_response(),
    }
}

/// Serve a matched embedded file with appropriate content type and caching.
fn serve_file(path: &str, file: &rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let body = file.data.to_vec();

    // Vite hashed assets get aggressive caching; everything else gets no-cache
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.as_ref()),
            (header::CACHE_CONTROL, cache),
        ],
        body,
    )
        .into_response()
}
