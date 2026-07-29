//! Dashboard payload and practice settings (T071).
//!
//! The dashboard answers two questions the vet has when opening the app: what still has to
//! go to the bookkeeper, and which stock is about to expire (FR-036). Settings are one row
//! that field-level auto-save patches (FR-037).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{Days, Utc};
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

/// A small in-memory PNG, the shape of an uploaded practice logo.
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbImage::from_pixel(120, 40, image::Rgb([27, 54, 93]));
    image::DynamicImage::ImageRgb8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encodes png");
    bytes
}

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

/// An accepted invoice, i.e. one waiting for the bookkeeper.
async fn accepted_invoice(app: &TestApp, pool: &PgPool) -> i64 {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(pool, appointment_id, patient_id).await;
    let service_id = common::seed_got_service(pool).await;

    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await;
    let id = invoice.id();
    let accepted = app
        .post(
            &format!("/api/invoices/{id}/accept"),
            json!({ "recipient_emails": [] }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    id
}

fn batch_numbers(lots: &Value) -> Vec<String> {
    lots.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["batch_number"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[sqlx::test]
async fn the_dashboard_counts_what_is_waiting_for_the_bookkeeper(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    let empty = app.get("/api/dashboard").await;
    assert_eq!(
        empty.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&empty.body)
    );
    assert_eq!(empty.json()["pending_invoice_count"], 0);

    let first = accepted_invoice(&app, &pool).await;
    accepted_invoice(&app, &pool).await;
    assert_eq!(
        app.get("/api/dashboard").await.json()["pending_invoice_count"],
        2
    );

    // Handing one over takes it off the list; cancelling never counts either.
    app.post_empty(&format!("/api/invoices/{first}/submit"))
        .await;
    assert_eq!(
        app.get("/api/dashboard").await.json()["pending_invoice_count"],
        1
    );
}

#[sqlx::test]
async fn the_dashboard_shows_the_five_soonest_expiring_lots_that_still_have_stock(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let today = Utc::now().date_naive();

    // Seven lots, deliberately out of order, plus one that is already used up. The seed
    // numbers each batch by its package count, so the batch number identifies the lot.
    let batch = |packages: i32| format!("BATCH-{}-{packages}", drug.packaging_id);
    for (index, days) in [90_u64, 10, 400, 30, 200, 5, 120].into_iter().enumerate() {
        let packages = i32::try_from(index).unwrap_or_default() + 1;
        common::seed_lot(
            &pool,
            drug.packaging_id,
            packages,
            dec("100"),
            today.checked_add_days(Days::new(days)),
        )
        .await;
    }
    // Expires tomorrow, but a correction already took the last of it off the shelf.
    let used_up = common::seed_lot(
        &pool,
        drug.packaging_id,
        8,
        dec("50"),
        today.checked_add_days(Days::new(1)),
    )
    .await;
    sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason)
         VALUES ($1, 'correction', -400, 'Verfallen und entsorgt')",
    )
    .bind(used_up)
    .execute(&pool)
    .await
    .expect("empty the lot");

    let lots = app.get("/api/dashboard").await.json()["expiring_lots"].clone();
    assert_eq!(
        lots.as_array().map(Vec::len),
        Some(5),
        "top five only: {lots:?}"
    );

    // Soonest first — and the empty lot is not stock the vet can still lose.
    assert_eq!(
        batch_numbers(&lots),
        vec![batch(6), batch(2), batch(4), batch(1), batch(7)],
        "5, 10, 30, 90 and 120 days out"
    );
    assert_eq!(lots[0]["remaining"], "600.00", "six packages of 100 ml");
    assert_eq!(lots[0]["drug_name"], "Amoxicillin 100");
    assert_eq!(lots[0]["unit"], "ml");
    assert!(
        lots[0]["lot_id"].is_i64(),
        "each entry is navigable: {lots:?}"
    );
}

#[sqlx::test]
async fn lots_without_an_expiration_date_are_not_expiring(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;

    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let lots = app.get("/api/dashboard").await.json()["expiring_lots"].clone();
    assert_eq!(lots, json!([]), "an undated lot cannot expire: {lots:?}");
}

#[sqlx::test]
async fn settings_start_empty_and_are_patched_field_by_field(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let initial = app.get("/api/settings").await;
    assert_eq!(
        initial.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&initial.body)
    );
    let initial = initial.json();
    assert_eq!(initial["practice_name"], "");
    assert_eq!(initial["cc_emails"], json!([]));
    assert!(initial["logo_attachment_id"].is_null());

    // Auto-save sends one field at a time; the others must stay as they are.
    let named = app
        .patch(
            "/api/settings",
            json!({ "practice_name": "Tierarztpraxis Musterfrau" }),
        )
        .await
        .json();
    assert_eq!(named["practice_name"], "Tierarztpraxis Musterfrau");

    let addressed = app
        .patch(
            "/api/settings",
            json!({ "practice_address": "Dorfstraße 1\n12345 Musterstadt", "iban": "DE02120300000000202051" }),
        )
        .await
        .json();
    assert_eq!(addressed["practice_name"], "Tierarztpraxis Musterfrau");
    assert_eq!(addressed["iban"], "DE02120300000000202051");
    assert_eq!(
        addressed["practice_address"],
        "Dorfstraße 1\n12345 Musterstadt"
    );

    // Still one row, whatever happens.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM global_settings")
        .fetch_one(&app.pool)
        .await
        .expect("count");
    assert_eq!(rows, 1);
}

#[sqlx::test]
async fn the_global_email_copies_are_validated(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let saved = app
        .patch(
            "/api/settings",
            json!({ "cc_emails": [" buchhaltung@example.com "], "bcc_emails": [] }),
        )
        .await
        .json();
    assert_eq!(
        saved["cc_emails"],
        json!(["buchhaltung@example.com"]),
        "addresses are stored trimmed"
    );

    let rejected = app
        .patch(
            "/api/settings",
            json!({ "bcc_emails": ["kein-at-zeichen"] }),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"bcc_emails".to_owned()));

    // The rejected patch changed nothing.
    assert_eq!(
        app.get("/api/settings").await.json()["bcc_emails"],
        json!([])
    );
}

#[sqlx::test]
async fn the_practice_logo_is_referenced_and_can_be_removed_again(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let uploaded = app
        .post_multipart("/api/attachments", "logo.png", "image/png", &png(), &[])
        .await;
    assert_eq!(
        uploaded.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&uploaded.body)
    );
    let attachment_id = uploaded.id();

    let with_logo = app
        .patch(
            "/api/settings",
            json!({ "logo_attachment_id": attachment_id }),
        )
        .await
        .json();
    assert_eq!(with_logo["logo_attachment_id"], attachment_id);

    let without_logo = app
        .patch("/api/settings", json!({ "logo_attachment_id": null }))
        .await
        .json();
    assert!(
        without_logo["logo_attachment_id"].is_null(),
        "the logo can be taken off again"
    );

    let missing = app
        .patch("/api/settings", json!({ "logo_attachment_id": 999_999 }))
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
}
