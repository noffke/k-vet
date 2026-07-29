//! Session authentication against a real Postgres session store (T016).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::{Method, StatusCode};
use common::{TEST_PASSWORD, TEST_USER, TestApp};
use sqlx::PgPool;

#[sqlx::test]
async fn wrong_password_is_rejected(pool: PgPool) {
    let app = TestApp::anonymous(pool).await;

    let response = app.login(TEST_USER, "wrong-password").await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/problem+json"),
        "errors are RFC 7807 problem documents"
    );
}

#[sqlx::test]
async fn unknown_user_is_rejected(pool: PgPool) {
    let app = TestApp::anonymous(pool).await;

    let response = app.login("somebody-else", TEST_PASSWORD).await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn protected_endpoints_require_a_session(pool: PgPool) {
    let app = TestApp::anonymous(pool).await;

    let response = app.get("/api/settings").await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn login_establishes_a_persisted_session(pool: PgPool) {
    let app = TestApp::anonymous(pool.clone()).await;

    let login = app.login(TEST_USER, TEST_PASSWORD).await;
    assert_eq!(login.status, StatusCode::OK);
    assert_eq!(login.json()["authenticated"], true);
    assert_eq!(login.json()["username"], TEST_USER);

    let session = app.get("/api/auth/session").await;
    assert_eq!(session.json()["authenticated"], true);

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM tower_sessions.session")
        .fetch_one(&pool)
        .await
        .expect("session table is readable");
    assert_eq!(
        stored, 1,
        "the session is stored in Postgres, not in memory"
    );
}

#[sqlx::test]
async fn logout_invalidates_the_session(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    assert_eq!(
        app.get("/api/auth/session").await.json()["authenticated"],
        true
    );

    let logout = app.send(Method::POST, "/api/auth/logout", None).await;
    assert_eq!(logout.status, StatusCode::OK);

    let after = app.get("/api/auth/session").await;
    assert_eq!(after.json()["authenticated"], false);

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM tower_sessions.session")
        .fetch_one(&pool)
        .await
        .expect("session table is readable");
    assert_eq!(stored, 0, "logout removes the stored session");
}

#[sqlx::test]
async fn session_survives_a_restart_of_the_binary(pool: PgPool) {
    let first = TestApp::new(pool.clone()).await;
    let cookie = first.session_cookie().expect("a session cookie was set");

    // A second app instance stands in for a restarted process on the same database.
    let restarted = TestApp::anonymous(pool).await;
    restarted.set_session_cookie(&cookie);

    let session = restarted.get("/api/auth/session").await;
    assert_eq!(
        session.json()["authenticated"],
        true,
        "Postgres-backed sessions outlive the process"
    );
}

#[sqlx::test]
async fn health_and_metrics_answer_without_a_session(pool: PgPool) {
    let app = TestApp::anonymous(pool).await;

    let health = app.get("/healthz").await;
    assert_eq!(health.status, StatusCode::OK);

    // The scrape endpoint is open on the appliance's own port (T079).
    let metrics = app.get("/metrics").await;
    assert_eq!(
        metrics.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&metrics.body)
    );
    let body = String::from_utf8_lossy(&metrics.body);
    assert!(
        body.contains("kvet_http_requests_total"),
        "requests are counted by route: {body}"
    );
    assert!(
        body.contains("route=\"/healthz\""),
        "the route label is the pattern, not the path: {body}"
    );
}
