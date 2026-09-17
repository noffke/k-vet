//! Prometheus metrics (T079).
//!
//! The appliance is scraped locally, so `/metrics` is plain text on the same port as the
//! app. Request counters and a latency histogram are labelled by route *pattern*, never by
//! the concrete path — otherwise every customer id would become its own time series.

use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// The recorder is process-global: installed once, however many apps the tests build.
static HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();

fn handle() -> Option<&'static PrometheusHandle> {
    HANDLE
        .get_or_init(|| match PrometheusBuilder::new().install_recorder() {
            Ok(handle) => Some(handle),
            Err(error) => {
                tracing::warn!(%error, "metrics recorder not installed");
                None
            }
        })
        .as_ref()
}

/// Installs the recorder at startup so the first scrape already has data.
pub fn install() {
    let _ = handle();
    // The Prometheus idiom for "which build is this": a gauge that is always 1, carrying the
    // answer in a label. With two instances scraped off one Pi it is how a graph says which
    // of them moved.
    gauge!("kvet_build_info", "version" => crate::version()).set(1.0);
}

/// Counts requests and records their duration, labelled by method, route and status.
pub async fn track(request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |path| path.as_str().to_owned());
    let method = request.method().to_string();
    let started = Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16().to_string();
    counter!(
        "kvet_http_requests_total",
        "method" => method.clone(),
        "route" => route.clone(),
        "status" => status,
    )
    .increment(1);
    histogram!(
        "kvet_http_request_duration_seconds",
        "method" => method,
        "route" => route,
    )
    .record(started.elapsed().as_secs_f64());

    response
}

/// The scrape endpoint.
pub async fn scrape() -> Response {
    match handle() {
        Some(handle) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            handle.render(),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics recorder not installed",
        )
            .into_response(),
    }
}
