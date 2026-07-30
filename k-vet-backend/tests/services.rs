//! Services: the imported GOT catalog, hidden filtering, GOT completeness, travel
//! expenses on a treatment line (T059).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

fn names(services: &Value) -> Vec<String> {
    services
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[sqlx::test]
async fn the_got_2022_catalog_is_imported_with_surgery_hidden(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // The one-time import migration provisions the whole fee schedule (FR-022).
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM service WHERE type = 'got'")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert!(
        total > 900,
        "the GOT 2022 schedule has ~930 positions, found {total}"
    );

    let complete: i64 =
        sqlx::query_scalar("SELECT count(*) FROM service WHERE type = 'got' AND draft")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(complete, 0, "imported positions are complete, not drafts");

    // A position from Teil A, searched by its number.
    let found = app.get("/api/services?q=1").await.json();
    assert!(
        names(&found)
            .iter()
            .any(|name| name.starts_with("Beratung im einzelnen Fall")),
        "position 1 is searchable by number: {found:?}"
    );

    // Surgical positions exist but stay hidden (logic.md).
    let hidden: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM service WHERE type = 'got' AND hidden AND name ILIKE '%Nephrotomie%'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(hidden, 1);
}

#[sqlx::test]
async fn hidden_services_are_excluded_from_lists_until_asked_for(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let visible = app.get("/api/services?q=Nephrotomie").await.json();
    assert_eq!(
        visible.as_array().map(Vec::len),
        Some(0),
        "hidden by default"
    );

    let with_hidden = app
        .get("/api/services?q=Nephrotomie&hidden=true")
        .await
        .json();
    assert_eq!(with_hidden.as_array().map(Vec::len), Some(1));
    assert_eq!(with_hidden[0]["hidden"], true);
}

#[sqlx::test]
async fn a_got_service_needs_its_number_and_factor_to_complete(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post("/api/services", json!({ "type": "got" })).await;
    let id = created.id();
    // The factor defaults to the single rate (100 %), so only the number is outstanding.
    assert_eq!(
        created.json()["missing_fields"],
        json!(["name", "vat_percent", "net_price", "got_number"])
    );
    assert_eq!(created.json()["factor"], "100.000");

    // Everything but the GOT fields: still a draft.
    let partial = app
        .patch(
            &format!("/api/services/{id}"),
            json!({ "name": "Sonderleistung", "vat_percent": "19.000", "net_price": "42.00" }),
        )
        .await
        .json();
    assert_eq!(partial["draft"], true);
    assert_eq!(partial["missing_fields"], json!(["got_number"]));

    let complete = app
        .patch(
            &format!("/api/services/{id}"),
            json!({ "got_number": "9999", "factor": "150.000" }),
        )
        .await
        .json();
    assert_eq!(complete["draft"], false);
    assert_eq!(
        complete["factor"], "150.000",
        "a GOT position may bill above the single rate"
    );
}

#[sqlx::test]
async fn a_self_defined_service_never_carries_a_got_number(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app
        .post("/api/services", json!({ "type": "self_defined" }))
        .await;
    let id = created.id();
    assert_eq!(
        created.json()["missing_fields"],
        json!(["name", "vat_percent", "net_price"]),
        "no GOT number or factor is required"
    );

    let complete = app
        .patch(
            &format!("/api/services/{id}"),
            json!({ "name": "Hausbesuch", "vat_percent": "19.000", "net_price": "25.00",
                    "got_number": "1234" }),
        )
        .await
        .json();

    assert_eq!(complete["draft"], false);
    assert!(
        complete["got_number"].is_null(),
        "a self-defined service stays without a number: {complete:?}"
    );
}

#[sqlx::test]
async fn a_zero_factor_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_got_service(&pool).await;

    let rejected = app
        .patch(&format!("/api/services/{id}"), json!({ "factor": "0" }))
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"factor".to_owned()));
}

#[sqlx::test]
async fn services_can_be_hidden_and_archived(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_got_service(&pool).await;

    app.patch(&format!("/api/services/{id}"), json!({ "hidden": true }))
        .await;
    let searched = app
        .get("/api/services?q=Allgemeine%20Untersuchung")
        .await
        .json();
    assert!(
        !names(&searched).contains(&"Allgemeine Untersuchung".to_owned()),
        "a hidden service leaves the default list"
    );

    app.patch(&format!("/api/services/{id}"), json!({ "hidden": false }))
        .await;
    app.post_empty(&format!("/api/services/{id}/archive")).await;
    let after_archive = app
        .get("/api/services?q=Allgemeine%20Untersuchung")
        .await
        .json();
    assert!(!names(&after_archive).contains(&"Allgemeine Untersuchung".to_owned()));

    app.post_empty(&format!("/api/services/{id}/unarchive"))
        .await;
    let restored = app
        .get("/api/services?q=Allgemeine%20Untersuchung")
        .await
        .json();
    assert!(names(&restored).contains(&"Allgemeine Untersuchung".to_owned()));
}

#[sqlx::test]
async fn a_travel_expense_line_is_priced_from_the_kilometres(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    // GOT § 10: 12 km × 3.50 = 42.00, not the catalog's 13.00.
    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1", "km": "12" }),
        )
        .await;
    assert_eq!(
        line.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&line.body)
    );
    let line = line.json();
    assert_eq!(line["km"], "12.00");
    assert_eq!(line["price_net"], "42.00");

    // A shorter trip falls back to the legal minimum.
    let item_id = line["id"].as_i64().unwrap_or_default();
    let short = app
        .patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "km": "2" }),
        )
        .await
        .json();
    assert_eq!(short["price_net"], "13.00");

    // Adverse travel conditions: ×2 on 12 km.
    let doubled = app
        .patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "km": "12", "km_multiplier": "2" }),
        )
        .await
        .json();
    assert_eq!(doubled["price_net"], "84.00");
    assert_eq!(doubled["line_net"], "84.00");
}

#[sqlx::test]
async fn an_ordinary_service_line_keeps_the_catalog_price(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_got_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    // Kilometres on a non-travel service change nothing about its price.
    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1", "km": "50" }),
        )
        .await
        .json();

    assert_eq!(line["price_net"], "23.62");
}
