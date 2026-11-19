//! The operator configuration the browser reads (issues.md: VAT preselect, default country).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
use crate::common;

use axum::http::StatusCode;
use common::TestApp;

#[tokio::test]
async fn the_defaults_are_the_german_ones() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn it_reports_what_the_operator_configured() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn an_unlabelled_instance_is_the_practice_s_own() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool).await;

    let body = app.get("/api/config").await.json();

    // Production sets nothing, so the browser has nothing to draw.
    assert!(body["environment_label"].is_null());
    // Outside an image there is no `KVET_VERSION`, and saying so beats inventing one.
    assert_eq!(body["version"], "dev");
}

#[tokio::test]
async fn a_labelled_instance_says_which_one_it_is() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::with_config(pool, |config| {
        config.server.environment_label = Some("TEST".to_owned());
    })
    .await;

    let body = app.get("/api/config").await.json();

    assert_eq!(body["environment_label"], "TEST");
}

#[tokio::test]
async fn a_blank_label_is_no_label() {
    let db = common::database().await;
    let pool = db.pool();
    // `environment_label = ""` is what an operator writes when they mean production, and it
    // must not draw an empty banner.
    let app = TestApp::with_config(pool, |config| {
        config.server.environment_label = Some("   ".to_owned());
    })
    .await;

    let body = app.get("/api/config").await.json();

    assert!(body["environment_label"].is_null());
}

#[tokio::test]
async fn it_needs_a_session() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::anonymous(pool).await;

    let response = app.get("/api/config").await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

/// What the sync app on the phone is set up with: off by default, and with `[dav] enabled` the
/// address, the login's name and the two collections — never a password.
#[tokio::test]
async fn the_phone_sync_is_described_only_when_enabled() {
    let db = common::database().await;
    let pool = db.pool();
    let off = TestApp::new(pool.clone()).await;
    assert!(off.get("/api/config").await.json()["dav"].is_null());

    let on = TestApp::with_config(pool, |config| {
        config.dav.enabled = true;
        config.server.base_url = "http://pi:8080/".to_owned();
    })
    .await;
    let dav = &on.get("/api/config").await.json()["dav"];
    assert_eq!(dav["url"], "http://pi:8080/dav/");
    assert_eq!(dav["username"], "tierarzt");
    assert_eq!(dav["calendar_name"], "Praxis-Termine");
    assert_eq!(dav["addressbook_name"], "Kunden");
    assert!(dav.get("password").is_none());
}

/// Every answer names the release that gave it, so an open tab notices a new one and offers to
/// reload: the API's, a refusal, the health probe, and the frontend the browser loads.
#[tokio::test]
async fn every_response_names_the_release_that_gave_it() {
    let db = common::database().await;
    let pool = db.pool();
    let signed_in = TestApp::new(pool.clone()).await;
    let anonymous = TestApp::anonymous(pool.clone()).await;
    let expected = k_vet_backend::version();

    let config = signed_in.get("/api/config").await;
    assert_eq!(config.status, StatusCode::OK);
    let refused = anonymous.get("/api/customers").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let health = anonymous.get("/healthz").await;
    assert_eq!(health.status, StatusCode::OK);
    // A client-side route, answered with the frontend's index.html (or, with none built, the
    // page saying so) — either way by the fallback that serves the app.
    let page = anonymous.get("/customers/1").await;

    for (what, response) in [
        ("the API", &config),
        ("a refusal", &refused),
        ("the health probe", &health),
        ("the app's page", &page),
    ] {
        assert_eq!(
            response.header(k_vet_backend::VERSION_HEADER),
            Some(expected),
            "{what} names the release"
        );
    }
}
