//! The invoice email templates offered when releasing or sending: configured in order, each
//! with whether its file is in the templates folder.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

/// The configured order is the order offered, the first being the default; a template whose
/// file is missing is listed as unavailable — and the app started all the same.
#[sqlx::test]
async fn the_templates_are_offered_in_order_with_their_files_checked(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let templates = app.get("/api/email-templates").await.json();
    assert_eq!(
        templates,
        json!([
            { "name": "Standard", "file": "invoice-email.txt", "available": true },
            { "name": "Kurz", "file": "invoice-email-kurz.txt", "available": true },
            { "name": "Fehlt", "file": "invoice-email-fehlt.txt", "available": false },
        ])
    );

    // Put in place, the missing file makes its template available — no restart.
    std::fs::write(
        app.templates.path().join("invoice-email-fehlt.txt"),
        "Rechnung {{ invoice_number }}\n\nText.\n",
    )
    .expect("the missing file");
    let templates = app.get("/api/email-templates").await.json();
    assert_eq!(templates[2]["available"], true);
}
