//! Dashboard payload and practice settings (T071).
//!
//! The dashboard answers the questions the vet has when opening the app: which money has not
//! arrived yet, what still has to go to the bookkeeper, and which stock is about to expire
//! (FR-036). Settings are one row that field-level auto-save patches (FR-037).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Days, Utc};
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

/// An invoice the customer already has, i.e. one waiting for the bookkeeper. Posting it is how
/// a test says "sent" without a mail server.
async fn sent_invoice(app: &TestApp, pool: &PgPool) -> i64 {
    let id = accepted_invoice(app, pool).await;
    let posted = app
        .post_empty(&format!("/api/invoices/{id}/mark-posted"))
        .await;
    assert_eq!(posted.status, StatusCode::OK);
    id
}

/// A visit on `when` with one animal and one billed position, and no invoice.
async fn worked_treatment(app: &TestApp, pool: &PgPool, when: DateTime<Utc>) -> i64 {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, when).await;
    let treatment_id = common::seed_treatment(pool, appointment_id, patient_id).await;
    let service_id = common::seed_got_service(pool).await;

    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    treatment_id
}

/// A released invoice that has not gone anywhere yet.
async fn accepted_invoice(app: &TestApp, pool: &PgPool) -> i64 {
    let treatment_id = worked_treatment(app, pool, Utc::now()).await;
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

    let first = sent_invoice(&app, &pool).await;
    sent_invoice(&app, &pool).await;
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
async fn the_dashboard_lists_the_three_ways_money_goes_missing(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let last_week = Utc::now() - Days::new(7);

    let empty = app.get("/api/dashboard").await;
    assert_eq!(
        empty.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&empty.body)
    );
    let empty = empty.json();
    for case in ["unbilled", "unreleased", "unsent"] {
        assert_eq!(empty[case]["count"], 0, "{case} starts empty");
        assert_eq!(empty[case]["entries"], json!([]));
    }

    // Worked, positions entered, and billed to nobody.
    let unbilled = worked_treatment(&app, &pool, last_week).await;

    // Billed, and the invoice never released.
    let unreleased = worked_treatment(&app, &pool, last_week).await;
    app.post(&format!("/api/treatments/{unreleased}/invoice"), json!({}))
        .await;

    // Released, and it never reached the customer — the email failed, or was never tried.
    let unsent = worked_treatment(&app, &pool, last_week).await;
    let invoice = app
        .post(&format!("/api/treatments/{unsent}/invoice"), json!({}))
        .await;
    let invoice_id = invoice.id();
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;

    let board = app.get("/api/dashboard").await.json();
    for (case, treatment_id) in [
        ("unbilled", unbilled),
        ("unreleased", unreleased),
        ("unsent", unsent),
    ] {
        assert_eq!(board[case]["count"], 1, "{case} holds exactly its own case");
        let entry = &board[case]["entries"][0];
        assert_eq!(entry["treatment_id"], treatment_id, "{case} leads there");
        assert_eq!(entry["customer_name"], "Erika Mustermann");
        assert_ne!(
            entry["total_gross"], "0.00",
            "{case} says how much is at stake"
        );
    }
    assert!(
        board["unbilled"]["entries"][0]["invoice_number"].is_null(),
        "there is no invoice to name yet"
    );
    assert!(!board["unsent"]["entries"][0]["invoice_number"].is_null());

    // Posting the released one settles it, and moves it on to the bookkeeper's pile.
    app.post_empty(&format!("/api/invoices/{invoice_id}/mark-posted"))
        .await;
    let board = app.get("/api/dashboard").await.json();
    assert_eq!(board["unsent"]["count"], 0);
    assert_eq!(board["pending_invoice_count"], 1);
}

#[sqlx::test]
async fn todays_unbilled_work_is_work_in_hand_and_not_yet_at_risk(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // Written up during the visit: positions, no invoice yet. A dashboard that shouts about
    // this teaches the vet to ignore it.
    worked_treatment(&app, &pool, Utc::now()).await;
    assert_eq!(
        app.get("/api/dashboard").await.json()["unbilled"]["count"],
        0
    );

    worked_treatment(&app, &pool, Utc::now() - Days::new(1)).await;
    assert_eq!(
        app.get("/api/dashboard").await.json()["unbilled"]["count"],
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
            json!({
                "practice_street": "Dorfstraße 1",
                "practice_zip": "12345",
                "practice_city": "Musterstadt",
                "practice_country": "DE",
                "email": "praxis@example.com",
                "iban": "DE02120300000000202051",
                "bic": "BYLADEM1001",
                "bank_name": "Musterbank",
            }),
        )
        .await
        .json();
    assert_eq!(addressed["practice_name"], "Tierarztpraxis Musterfrau");
    assert_eq!(addressed["iban"], "DE02120300000000202051");
    assert_eq!(addressed["practice_street"], "Dorfstraße 1");
    assert_eq!(addressed["practice_zip"], "12345");
    assert_eq!(addressed["practice_city"], "Musterstadt");
    assert_eq!(addressed["practice_country"], "DE");
    assert_eq!(addressed["email"], "praxis@example.com");
    assert_eq!(addressed["bic"], "BYLADEM1001");
    assert_eq!(addressed["bank_name"], "Musterbank");

    // A BIC that is not 8 or 11 characters would silently break the GiroCode.
    let rejected = app.patch("/api/settings", json!({ "bic": "NOPE" })).await;
    assert_eq!(
        rejected.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a malformed BIC is refused"
    );

    // Country validation is a real ISO 3166-1 lookup, not a shape check.
    let bogus = app
        .patch("/api/settings", json!({ "practice_country": "XX" }))
        .await;
    assert_eq!(
        bogus.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "XX is not a country"
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
