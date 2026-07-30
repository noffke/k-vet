//! Serves the built frontend from `server.web_dir` with SPA fallback.
//!
//! The release image copies `k-vet-web/dist` into the image and points `web_dir` at it; in
//! development the Vite dev server serves the UI and proxies `/api` to this process.

use std::path::{Component, Path, PathBuf};

use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::api::attachments::guess_mime;

/// The entry document; also the fallback for client-side routes.
const INDEX: &str = "index.html";

/// What a request path maps to inside `web_dir`.
#[derive(Debug, PartialEq, Eq)]
enum Resolved {
    /// A file to read, plus the caching it may claim.
    File { relative: PathBuf, cache: Cache },
    /// The path tried to leave `web_dir`.
    Rejected,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Cache {
    /// Vite emits content-hashed names below `assets/`, so those never change.
    Immutable,
    Revalidate,
}

impl Cache {
    fn header(self) -> &'static str {
        match self {
            Self::Immutable => "public, max-age=31536000, immutable",
            Self::Revalidate => "no-cache",
        }
    }
}

/// Maps a request path to a file below `web_dir`, refusing anything that would escape it.
fn resolve(uri_path: &str) -> Resolved {
    let trimmed = uri_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Resolved::File {
            relative: PathBuf::from(INDEX),
            cache: Cache::Revalidate,
        };
    }

    // Percent-encoded paths are refused outright: the built frontend never emits them, and
    // `%2e%2e` must not turn into `..` if anything downstream ever decodes the path.
    if trimmed.contains('%') {
        return Resolved::Rejected;
    }

    let candidate = Path::new(trimmed);
    // Only plain names: no `..`, no root, no prefixes — a served directory must never become
    // a window onto the rest of the filesystem.
    if !candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Resolved::Rejected;
    }

    let cache = if trimmed.starts_with("assets/") {
        Cache::Immutable
    } else {
        Cache::Revalidate
    };
    Resolved::File {
        relative: candidate.to_path_buf(),
        cache,
    }
}

/// Fallback handler: a static file, else `index.html` so a reloaded client route still works.
pub async fn handler(State(state): State<AppState>, uri: Uri) -> Response {
    let web_dir = state.config.server.web_dir.as_path();
    let Resolved::File { relative, cache } = resolve(uri.path()) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    match tokio::fs::read(web_dir.join(&relative)).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (
                    header::CONTENT_TYPE,
                    guess_mime(&relative.to_string_lossy()),
                ),
                (header::CACHE_CONTROL, cache.header().to_owned()),
            ],
            bytes,
        )
            .into_response(),
        // An unknown path is a client-side route: hand the SPA its entry document.
        Err(_) if relative != Path::new(INDEX) => serve_index(web_dir).await,
        Err(_) => missing_frontend(web_dir),
    }
}

async fn serve_index(web_dir: &Path) -> Response {
    match tokio::fs::read(web_dir.join(INDEX)).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8".to_owned()),
                (header::CACHE_CONTROL, Cache::Revalidate.header().to_owned()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => missing_frontend(web_dir),
    }
}

/// No frontend on disk — a misconfigured deployment, or a developer running only the API.
fn missing_frontend(web_dir: &Path) -> Response {
    tracing::warn!(web_dir = %web_dir.display(), "no frontend found");
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned())],
        format!(
            "no frontend at {} — run `npm run dev` in k-vet-web/ for development, \
             or point `server.web_dir` at a built `k-vet-web/dist`",
            web_dir.display()
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Resolved {
        Resolved::File {
            relative: PathBuf::from(path),
            cache: Cache::Revalidate,
        }
    }

    #[test]
    fn the_root_serves_the_entry_document() {
        assert_eq!(resolve("/"), file(INDEX));
        assert_eq!(resolve(""), file(INDEX));
    }

    #[test]
    fn a_client_route_resolves_to_a_file_that_will_miss() {
        // Neither path exists on disk, so the handler falls back to index.html.
        assert_eq!(resolve("/invoices"), file("invoices"));
        assert_eq!(resolve("/treatments/42"), file("treatments/42"));
    }

    #[test]
    fn hashed_assets_are_cached_forever_and_the_shell_is_not() {
        assert_eq!(
            resolve("/assets/index-a1b2c3.js"),
            Resolved::File {
                relative: PathBuf::from("assets/index-a1b2c3.js"),
                cache: Cache::Immutable,
            }
        );
        assert_eq!(resolve("/favicon.svg"), file("favicon.svg"));
        assert_eq!(
            Cache::Immutable.header(),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(Cache::Revalidate.header(), "no-cache");
    }

    #[test]
    fn paths_that_would_leave_the_directory_are_refused() {
        for path in [
            "/../config.toml",
            "/assets/../../etc/passwd",
            "/..",
            "/a/../..",
            // Encoded, in case something downstream ever decodes before we look.
            "/%2e%2e/config.toml",
            "/assets/%2E%2E/%2E%2E/etc/passwd",
        ] {
            assert_eq!(resolve(path), Resolved::Rejected, "{path} must be refused");
        }
    }

    #[test]
    fn stacked_slashes_stay_inside_the_directory() {
        // Leading slashes collapse, so this is a plain relative path below web_dir — it
        // will simply not exist and fall through to the SPA.
        assert_eq!(resolve("//etc/passwd"), file("etc/passwd"));
    }
}
