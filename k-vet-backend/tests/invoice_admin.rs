//! Invoice overview and the bookkeeping hand-off: list ordering and filters, submit,
//! bulk-submit, and the ZIP of all pending PDFs (T067).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use std::io::Cursor;

use axum::http::{StatusCode, header};
use chrono::Utc;
use common::{TestApp, TestResponse};
use serde_json::{Value, json};
use sqlx::PgPool;
use zip::ZipArchive;

/// One billable treatment with a single service line, plus its fresh invoice.
async fn invoice(app: &TestApp, pool: &PgPool) -> Value {
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

    let created = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await;
    assert_eq!(
        created.status,
        StatusCode::OK,
        "invoice creation failed: {}",
        String::from_utf8_lossy(&created.body)
    );
    created.json()
}

/// An invoice waiting for the bookkeeper: accepted, not yet submitted.
async fn accepted_invoice(app: &TestApp, pool: &PgPool) -> Value {
    let invoice = invoice(app, pool).await;
    let id = invoice["id"].as_i64().unwrap_or_default();
    let accepted = app
        .post(
            &format!("/api/invoices/{id}/accept"),
            json!({ "recipient_emails": [] }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    accepted.json()
}

fn numbers(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["invoice_number"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn number_of(invoice: &Value) -> String {
    invoice["invoice_number"]
        .as_str()
        .expect("an invoice always has a number")
        .to_owned()
}

fn zip_entries(response: &TestResponse) -> Vec<String> {
    let mut archive =
        ZipArchive::new(Cursor::new(response.body.clone())).expect("the body is a ZIP archive");
    (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("entry")
                .name()
                .trim_start_matches('/')
                .to_owned()
        })
        .collect()
}

#[sqlx::test]
async fn pending_invoices_come_first_and_cancelled_ones_stay_out_of_the_list(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // Created first, so it is the oldest by id — ordering must not fall back to that.
    let created = invoice(&app, &pool).await;
    let pending = accepted_invoice(&app, &pool).await;
    let submitted = accepted_invoice(&app, &pool).await;
    let cancelled = accepted_invoice(&app, &pool).await;

    app.post_empty(&format!(
        "/api/invoices/{}/submit",
        submitted["id"].as_i64().unwrap_or_default()
    ))
    .await;
    app.post_empty(&format!(
        "/api/invoices/{}/cancel",
        cancelled["id"].as_i64().unwrap_or_default()
    ))
    .await;

    let list = app.get("/api/invoices").await;
    assert_eq!(
        list.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&list.body)
    );
    let list = list.json();
    let listed = numbers(&list);

    // Waiting for the bookkeeper is what the vet came for, so it is on top (FR-034).
    assert_eq!(
        listed.first(),
        Some(&number_of(&pending)),
        "listed: {listed:?}"
    );
    assert!(listed.contains(&number_of(&created)));
    assert!(listed.contains(&number_of(&submitted)));
    assert!(
        !listed.contains(&number_of(&cancelled)),
        "cancelled invoices are excluded by default: {listed:?}"
    );

    // Asked for explicitly, they are there — a cancellation stays auditable.
    let with_cancelled = app.get("/api/invoices?cancelled=true").await.json();
    assert!(numbers(&with_cancelled).contains(&number_of(&cancelled)));

    // The pending filter is what the dashboard count and the bundle are built on.
    let only_pending = app.get("/api/invoices?pending=true").await.json();
    assert_eq!(numbers(&only_pending), vec![number_of(&pending)]);
}

#[sqlx::test]
async fn the_list_can_be_searched_by_number_and_customer_name(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let target = accepted_invoice(&app, &pool).await;
    let _other = accepted_invoice(&app, &pool).await;

    let by_number = app
        .get(&format!("/api/invoices?q={}", number_of(&target)))
        .await
        .json();
    assert_eq!(numbers(&by_number), vec![number_of(&target)]);

    // seed_customer creates "Erika Mustermann"; both invoices belong to such a customer.
    let by_name = app.get("/api/invoices?q=Mustermann").await.json();
    assert_eq!(by_name.as_array().map(Vec::len), Some(2));
    assert_eq!(by_name[0]["customer_name"], "Erika Mustermann");
}

#[sqlx::test]
async fn submitting_stamps_the_hand_off_and_happens_once(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let invoice = accepted_invoice(&app, &pool).await;
    let id = invoice["id"].as_i64().unwrap_or_default();
    assert!(invoice["ts_submitted"].is_null());

    let submitted = app.post_empty(&format!("/api/invoices/{id}/submit")).await;
    assert_eq!(
        submitted.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&submitted.body)
    );
    let submitted = submitted.json();
    assert_eq!(submitted["status"], "submitted");
    assert!(
        !submitted["ts_submitted"].is_null(),
        "FR-035 wants the timestamp"
    );
    // The earlier steps keep their stamps: the trail grows, it never gets rewritten.
    assert!(!submitted["ts_accepted"].is_null());

    let again = app.post_empty(&format!("/api/invoices/{id}/submit")).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn only_an_accepted_invoice_can_be_submitted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    let created = invoice(&app, &pool).await;
    let created_id = created["id"].as_i64().unwrap_or_default();
    let rejected = app
        .post_empty(&format!("/api/invoices/{created_id}/submit"))
        .await;
    assert_eq!(
        rejected.status,
        StatusCode::CONFLICT,
        "a draft-stage invoice never reaches the bookkeeper"
    );

    let cancelled = accepted_invoice(&app, &pool).await;
    let cancelled_id = cancelled["id"].as_i64().unwrap_or_default();
    app.post_empty(&format!("/api/invoices/{cancelled_id}/cancel"))
        .await;
    let rejected = app
        .post_empty(&format!("/api/invoices/{cancelled_id}/submit"))
        .await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn bulk_submit_hands_over_every_pending_invoice_at_once(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let first = accepted_invoice(&app, &pool).await;
    let second = accepted_invoice(&app, &pool).await;
    let untouched = invoice(&app, &pool).await;

    let result = app.post_empty("/api/invoices/bulk-submit").await;
    assert_eq!(
        result.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&result.body)
    );
    let result = result.json();
    assert_eq!(result["submitted"], 2);
    let handed_over = result["invoice_numbers"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(handed_over.contains(&number_of(&first)));
    assert!(handed_over.contains(&number_of(&second)));

    // Nothing is left pending, and the created invoice was not swept along.
    assert_eq!(
        app.get("/api/invoices?pending=true").await.json(),
        json!([])
    );
    let created_again = app
        .get(&format!(
            "/api/invoices/{}",
            untouched["id"].as_i64().unwrap_or_default()
        ))
        .await
        .json();
    assert_eq!(created_again["status"], "created");

    // A second run has nothing to do — the monthly chore is safe to repeat.
    let empty = app.post_empty("/api/invoices/bulk-submit").await.json();
    assert_eq!(empty["submitted"], 0);
}

#[sqlx::test]
async fn the_pending_bundle_zips_one_pdf_per_waiting_invoice(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let first = accepted_invoice(&app, &pool).await;
    let second = accepted_invoice(&app, &pool).await;
    let _not_accepted = invoice(&app, &pool).await;

    let bundle = app.get("/api/invoices/pending-pdfs").await;
    assert_eq!(
        bundle.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bundle.body)
    );
    assert_eq!(
        bundle.header(header::CONTENT_TYPE),
        Some("application/zip"),
        "the browser must offer it as one download"
    );
    assert!(
        bundle
            .header(header::CONTENT_DISPOSITION)
            .unwrap_or_default()
            .contains("attachment"),
        "a bundle is saved, not rendered"
    );

    let entries = zip_entries(&bundle);
    assert_eq!(entries.len(), 2, "only the pending ones: {entries:?}");
    assert!(entries.contains(&format!("Rechnung-{}.pdf", number_of(&first))));
    assert!(entries.contains(&format!("Rechnung-{}.pdf", number_of(&second))));

    // Downloading does not hand anything over — that is the separate, explicit step.
    let still_pending = app.get("/api/invoices?pending=true").await.json();
    assert_eq!(still_pending.as_array().map(Vec::len), Some(2));
}

#[sqlx::test]
async fn an_empty_bundle_says_so_instead_of_shipping_an_empty_zip(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let bundle = app.get("/api/invoices/pending-pdfs").await;
    assert_eq!(
        bundle.status,
        StatusCode::CONFLICT,
        "nothing pending is a message, not a download"
    );
}
