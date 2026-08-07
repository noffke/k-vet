//! The operator configuration the browser reads (issues.md: VAT preselect, default country).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use sqlx::PgPool;

#[sqlx::test]
async fn the_defaults_are_the_german_ones(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let response = app.get("/api/config").await;

    assert_eq!(response.status, StatusCode::OK);
    let body = response.json();
    assert_eq!(body["currency"], "EUR");
    assert_eq!(body["default_country"], "DE");
    // The order is the operator's: the first rate is what a new record starts on. The scale
    // is the column's, so the browser can match a rate against a stored `vat_percent`.
    assert_eq!(body["vat_rates"][0], "19.000");
    assert_eq!(body["vat_rates"][1], "7.000");
}

#[sqlx::test]
async fn it_reports_what_the_operator_configured(pool: PgPool) {
    let app = TestApp::with_config(pool, |config| {
        config.invoice.currency = "CHF".to_owned();
        config.invoice.default_country = "at".to_owned();
    })
    .await;

    let body = app.get("/api/config").await.json();

    assert_eq!(body["currency"], "CHF");
    // Stored as typed, answered as ISO 3166-1 writes it.
    assert_eq!(body["default_country"], "AT");
}

#[sqlx::test]
async fn it_needs_a_session(pool: PgPool) {
    let app = TestApp::anonymous(pool).await;

    let response = app.get("/api/config").await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}
