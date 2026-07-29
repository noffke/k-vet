//! Serves the built frontend from inside the binary (rust-embed) with SPA fallback.
//!
//! The `embed-frontend` feature is enabled for release builds; in development the Vite
//! dev server serves the UI and proxies `/api` to this process.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[cfg(feature = "embed-frontend")]
#[derive(rust_embed::Embed)]
#[folder = "../k-vet-web/dist"]
struct Assets;

/// Fallback handler: static asset, else `index.html` so client-side routes work on reload.
pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    serve(if path.is_empty() { "index.html" } else { path })
}

#[cfg(feature = "embed-frontend")]
fn serve(path: &str) -> Response {
    match Assets::get(path) {
        Some(file) => {
            let mime = file.metadata.mimetype();
            // Vite emits content-hashed asset names, so everything below /assets is immutable.
            let cache = if path.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
                file.data,
            )
                .into_response()
        }
        None if path != "index.html" => serve("index.html"),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[cfg(not(feature = "embed-frontend"))]
fn serve(_path: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "frontend not embedded in this build — run `npm run dev` in k-vet-web/ \
         or rebuild with `--features embed-frontend`",
    )
        .into_response()
}
