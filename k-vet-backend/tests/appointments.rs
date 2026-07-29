//! Appointments: draft rules, auto-save PATCH, duplication, delete guard (T030).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Local, Utc};
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn a_new_appointment_starts_as_an_empty_draft(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post("/api/appointments", json!({})).await;

    assert_eq!(created.status, StatusCode::OK);
    let body = created.json();
    assert_eq!(
        body["draft"], true,
        "auto-save needs a row before anything is typed"
    );
    assert_eq!(body["missing_fields"][0], "starts_at");
    assert!(body["starts_at"].is_null());
}

#[sqlx::test]
async fn filling_the_time_completes_the_appointment(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let created = app.post("/api/appointments", json!({})).await;

    let patched = app
        .patch(
            &format!("/api/appointments/{}", created.id()),
            json!({ "starts_at": "2026-05-04T09:30:00Z", "note": "Hausbesuch" }),
        )
        .await
        .json();

    assert_eq!(
        patched["draft"], false,
        "the draft flag flips without a save action"
    );
    assert_eq!(patched["missing_fields"].as_array().map(Vec::len), Some(0));
    assert_eq!(patched["note"], "Hausbesuch");
}

#[sqlx::test]
async fn clearing_the_time_of_a_complete_appointment_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_appointment(&pool, Utc::now()).await;

    let response = app
        .patch(
            &format!("/api/appointments/{id}"),
            json!({ "starts_at": null }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.error_fields().contains(&"starts_at".to_owned()));

    // The last valid value is still stored.
    let unchanged = app.get(&format!("/api/appointments/{id}")).await.json();
    assert!(!unchanged["starts_at"].is_null());
}

#[sqlx::test]
async fn the_note_can_be_cleared(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let created = app
        .post(
            "/api/appointments",
            json!({ "starts_at": "2026-05-04T09:30:00Z", "note": "x" }),
        )
        .await;

    let cleared = app
        .patch(
            &format!("/api/appointments/{}", created.id()),
            json!({ "note": null }),
        )
        .await
        .json();

    assert!(cleared["note"].is_null());
    assert_eq!(
        cleared["draft"], false,
        "an optional field never affects completeness"
    );
}

#[sqlx::test]
async fn the_list_shows_newest_first_with_treatment_counts(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let older = common::seed_appointment(
        &pool,
        "2026-05-01T09:00:00Z".parse::<DateTime<Utc>>().unwrap(),
    )
    .await;
    let newer = common::seed_appointment(
        &pool,
        "2026-05-04T09:00:00Z".parse::<DateTime<Utc>>().unwrap(),
    )
    .await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    common::seed_treatment(&pool, newer, patient_id).await;

    let list = app.get("/api/appointments").await.json();

    assert_eq!(list[0]["id"], newer);
    assert_eq!(list[0]["treatment_count"], 1);
    assert_eq!(list[1]["id"], older);
    assert_eq!(list[1]["treatment_count"], 0);
}

#[sqlx::test]
async fn duplicating_moves_the_appointment_to_today_and_copies_its_treatments(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let source = common::seed_appointment(
        &pool,
        "2026-01-15T14:45:00Z".parse::<DateTime<Utc>>().unwrap(),
    )
    .await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let treatment_id = common::seed_treatment(&pool, source, patient_id).await;
    let service_id = common::seed_got_service(&pool).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;

    let copy = app
        .post(
            &format!("/api/appointments/{source}/duplicate"),
            json!({ "price_mode": "verbatim" }),
        )
        .await;
    assert_eq!(
        copy.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&copy.body)
    );
    let copy = copy.json();

    assert_ne!(copy["id"], source);
    assert_eq!(copy["treatment_count"], 1, "treatments come along");
    assert_eq!(copy["draft"], false);

    let starts_at: DateTime<Utc> = copy["starts_at"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .expect("the copy has a start time");
    assert_eq!(
        starts_at.with_timezone(&Local).date_naive(),
        Local::now().date_naive(),
        "the duplicate's date defaults to today"
    );

    let treatments = app
        .get(&format!("/api/appointments/{}/treatments", copy["id"]))
        .await
        .json();
    let items = app
        .get(&format!("/api/treatments/{}/items", treatments[0]["id"]))
        .await
        .json();
    assert_eq!(
        items[0]["price_gross"], "23.62",
        "prices are copied verbatim"
    );
    assert_eq!(items[0]["patient_id"], patient_id);
}

#[sqlx::test]
async fn an_appointment_can_be_deleted_while_it_is_not_invoiced(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_appointment(&pool, Utc::now()).await;

    assert_eq!(
        app.delete(&format!("/api/appointments/{id}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&format!("/api/appointments/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
}
