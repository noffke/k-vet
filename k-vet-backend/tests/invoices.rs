//! Invoice lifecycle: numbers, PDF, freeze on accept, burned numbers, reversals (T026).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{Datelike, Local, Utc};
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

struct Billed {
    treatment_id: i64,
    lot_id: i64,
    packaging_id: i64,
}

/// A treatment with one GOT service line and one drug line dispensing a 10 ml subset.
async fn billable_treatment(app: &TestApp, pool: &PgPool) -> Billed {
    let customer_id = common::seed_customer(pool).await;
    common::seed_customer_email(pool, customer_id, "kundin@example.com").await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(pool, appointment_id, patient_id).await;

    let service_id = common::seed_got_service(pool).await;
    let drug = common::seed_drug(pool).await;
    let lot_id = common::seed_lot(pool, drug.packaging_id, 1, dec("100"), None).await;

    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "1"
        }),
    )
    .await;

    Billed {
        treatment_id,
        lot_id,
        packaging_id: drug.packaging_id,
    }
}

async fn create_invoice(app: &TestApp, treatment_id: i64) -> Value {
    let response = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({ "includes_finding": true, "note": "Bitte innerhalb von 14 Tagen zahlen." }),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "invoice creation failed: {}",
        String::from_utf8_lossy(&response.body)
    );
    response.json()
}

#[sqlx::test]
async fn creating_an_invoice_allocates_a_number_and_renders_a_pdf(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;

    let invoice = create_invoice(&app, billed.treatment_id).await;

    assert_eq!(invoice["status"], "created");
    assert_eq!(
        invoice["invoice_number"],
        format!("{}-0001", Local::now().year()),
        "the configured pattern is {{year}}-{{counter:4}}"
    );
    assert_eq!(invoice["includes_finding"], true);
    assert_eq!(
        invoice["total_gross"], "31.09",
        "VAT per line: 4,49 on 23,62 plus 0,48 on 2,50 = 4,97 on a net of 26,12. Rounding on the
         group total would give 4,96 and a 31,08 total — a cent under the 28,11 + 2,98 the invoice
         actually prints.",
    );
    assert!(
        invoice["pdf_attachment_id"].is_i64(),
        "the PDF is attached for inspection"
    );

    let pdf = app
        .get(&format!("/api/invoices/{}/pdf", invoice["id"]))
        .await;
    assert_eq!(pdf.status, StatusCode::OK);
    assert_eq!(pdf.content_type.as_deref(), Some("application/pdf"));
    assert!(pdf.body.starts_with(b"%PDF-"));
}

#[sqlx::test]
async fn updating_a_created_invoice_keeps_its_number(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let first = create_invoice(&app, billed.treatment_id).await;

    let updated = app
        .post(
            &format!("/api/treatments/{}/invoice", billed.treatment_id),
            json!({ "includes_finding": false }),
        )
        .await
        .json();

    assert_eq!(updated["id"], first["id"], "the same invoice is updated");
    assert_eq!(updated["invoice_number"], first["invoice_number"]);
    assert_eq!(updated["includes_finding"], false);

    let invoices: i64 = sqlx::query_scalar("SELECT count(*) FROM invoice")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(invoices, 1, "no number was burned");
}

#[sqlx::test]
async fn accepting_freezes_the_treatment(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();

    let accepted = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": ["kundin@example.com"] }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    let accepted = accepted.json();
    assert_eq!(accepted["status"], "accepted");
    assert!(
        !accepted["ts_accepted"].is_null(),
        "the acceptance is timestamped"
    );
    assert_eq!(accepted["email_recipients"][0], "kundin@example.com");
    // No SMTP server in tests: the send fails, the invoice stays accepted and retriable.
    assert!(accepted["ts_sent_email"].is_null());

    // The stock movements are frozen: lines can no longer be changed.
    let items = app
        .get(&format!("/api/treatments/{}/items", billed.treatment_id))
        .await
        .json();
    let drug_item = items[1]["id"].as_i64().unwrap_or_default();
    let rejected = app
        .patch(
            &format!("/api/treatment-items/{drug_item}"),
            json!({ "quantity": "20" }),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);
    assert_eq!(common::lot_remaining(&pool, billed.lot_id).await, dec("90"));

    let treatment = app
        .get(&format!("/api/treatments/{}", billed.treatment_id))
        .await
        .json();
    assert_eq!(treatment["frozen"], true);
}

#[sqlx::test]
async fn updating_an_accepted_invoice_burns_its_number_and_reverses_the_stock(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let first = create_invoice(&app, billed.treatment_id).await;
    let first_id = first["id"].as_i64().unwrap_or_default();
    app.post(
        &format!("/api/invoices/{first_id}/accept"),
        json!({ "recipient_emails": ["kundin@example.com"] }),
    )
    .await;

    let replacement = app
        .post(
            &format!("/api/treatments/{}/invoice", billed.treatment_id),
            json!({ "includes_finding": true }),
        )
        .await;
    assert_eq!(replacement.status, StatusCode::OK);
    let replacement = replacement.json();

    assert_ne!(replacement["id"], first["id"], "a new invoice is created");
    assert_ne!(
        replacement["invoice_number"], first["invoice_number"],
        "the accepted invoice's number is burned"
    );
    assert_eq!(replacement["status"], "created");

    let cancelled = app.get(&format!("/api/invoices/{first_id}")).await.json();
    assert_eq!(cancelled["status"], "cancelled");
    assert!(!cancelled["ts_cancelled"].is_null());

    // The frozen dispense was compensated, so the lot is whole again ...
    assert_eq!(
        common::lot_remaining(&pool, billed.lot_id).await,
        dec("100")
    );
    // ... and the history was not rewritten: dispense plus linked correction.
    let movements: Vec<(String, Decimal, Option<i64>)> = sqlx::query_as(
        "SELECT kind::text, quantity, reverses_movement_id
         FROM drug_stock_movement WHERE lot_id = $1 ORDER BY id",
    )
    .bind(billed.lot_id)
    .fetch_all(&pool)
    .await
    .expect("movements");
    assert_eq!(movements.len(), 2, "append-only: {movements:?}");
    assert_eq!(movements[0].0, "dispense");
    assert_eq!(movements[0].1, dec("-10"), "10 ml were dispensed");
    assert_eq!(movements[1].0, "correction");
    assert_eq!(movements[1].1, dec("10"));
    assert!(
        movements[1].2.is_some(),
        "the correction links the movement it reverses"
    );

    // The treatment is editable again (its new invoice is only `created`).
    let items = app
        .get(&format!("/api/treatments/{}/items", billed.treatment_id))
        .await
        .json();
    let drug_item = items[1]["id"].as_i64().unwrap_or_default();
    let patched = app
        .patch(
            &format!("/api/treatment-items/{drug_item}"),
            json!({ "quantity": "2" }),
        )
        .await;
    assert_eq!(
        patched.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&patched.body)
    );
    assert_eq!(common::lot_remaining(&pool, billed.lot_id).await, dec("80"));
}

#[sqlx::test]
async fn cancelling_an_accepted_invoice_restores_the_stock(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": ["kundin@example.com"] }),
    )
    .await;

    let cancelled = app
        .post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
        .await;
    assert_eq!(cancelled.status, StatusCode::OK);
    assert_eq!(cancelled.json()["status"], "cancelled");

    assert_eq!(
        common::lot_remaining(&pool, billed.lot_id).await,
        dec("100"),
        "initial quantity plus all movements equals the remaining stock (SC-007)"
    );

    // Cancelling twice is refused rather than writing a second reversal.
    let again = app
        .post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn cancelling_a_created_invoice_leaves_the_draft_dispenses_alone(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;

    app.post_empty(&format!("/api/invoices/{}/cancel", invoice["id"]))
        .await;

    // Draft dispenses were never booked, so there is nothing to compensate.
    let corrections: i64 =
        sqlx::query_scalar("SELECT count(*) FROM drug_stock_movement WHERE kind = 'correction'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(corrections, 0);
    assert_eq!(common::lot_remaining(&pool, billed.lot_id).await, dec("90"));
}

#[sqlx::test]
async fn numbers_are_monotonic_and_never_reused(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let year = Local::now().year();

    let mut numbers = Vec::new();
    for _ in 0..3 {
        let billed = billable_treatment(&app, &pool).await;
        let invoice = create_invoice(&app, billed.treatment_id).await;
        let invoice_id = invoice["id"].as_i64().unwrap_or_default();
        numbers.push(
            invoice["invoice_number"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        );
        // Accept and cancel: the number is burned.
        app.post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": ["kundin@example.com"] }),
        )
        .await;
        app.post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
            .await;
    }

    assert_eq!(
        numbers,
        vec![
            format!("{year}-0001"),
            format!("{year}-0002"),
            format!("{year}-0003"),
        ]
    );

    // Every burned number belongs to exactly one cancelled invoice (SC-006).
    let cancelled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM invoice WHERE status = 'cancelled'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(cancelled, 3);
    let distinct: i64 = sqlx::query_scalar("SELECT count(DISTINCT invoice_number) FROM invoice")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(distinct, 3);

    // The counter row is the monotonic source of truth.
    let counter: i64 = sqlx::query_scalar("SELECT counter FROM invoice_number_sequence")
        .fetch_one(&pool)
        .await
        .expect("counter");
    assert_eq!(counter, 3);
}

#[sqlx::test]
async fn a_treatment_without_lines_cannot_be_invoiced(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    let response = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn only_a_created_invoice_can_be_accepted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();

    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": ["kundin@example.com"] }),
    )
    .await;
    let twice = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": ["kundin@example.com"] }),
        )
        .await;

    assert_eq!(twice.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn one_live_invoice_per_treatment_is_enforced_by_the_database(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    create_invoice(&app, billed.treatment_id).await;

    // Bypassing the API: the partial unique index is the real guarantee.
    let result = sqlx::query(
        "INSERT INTO invoice (treatment_id, invoice_number, invoice_date)
         VALUES ($1, 'MANUELL-1', current_date)",
    )
    .bind(billed.treatment_id)
    .execute(&pool)
    .await;

    assert!(result.is_err(), "a second live invoice must be impossible");
}

#[sqlx::test]
async fn a_dispensed_treatment_cannot_be_deleted_once_invoiced(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    create_invoice(&app, billed.treatment_id).await;

    let rejected = app
        .delete(&format!("/api/treatments/{}", billed.treatment_id))
        .await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);

    let appointment: i64 = sqlx::query_scalar("SELECT appointment_id FROM treatment WHERE id = $1")
        .bind(billed.treatment_id)
        .fetch_one(&pool)
        .await
        .expect("appointment id");
    let rejected = app
        .delete(&format!("/api/appointments/{appointment}"))
        .await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);

    // Unused packagings stay available for other treatments.
    assert!(billed.packaging_id > 0);
}

#[sqlx::test]
async fn sending_needs_a_released_invoice_and_an_address_to_send_to(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();

    let too_early = app
        .post(
            &format!("/api/invoices/{invoice_id}/send"),
            json!({ "recipient_emails": ["kundin@example.com"] }),
        )
        .await;
    assert_eq!(too_early.status, StatusCode::CONFLICT);

    // Accepted with nobody ticked — which used to make the invoice unsendable for good,
    // because only `accept` could ever write the recipients.
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;
    let without_recipients = app
        .post(
            &format!("/api/invoices/{invoice_id}/send"),
            json!({ "recipient_emails": [] }),
        )
        .await;
    assert_eq!(without_recipients.status, StatusCode::UNPROCESSABLE_ENTITY);

    // With an address it gets that far: no SMTP server in tests, so the mail server rejects it
    // and the invoice stays accepted — which is exactly what the dashboard calls "not sent".
    //
    // 502 rather than 500, carrying a key: an unreachable relay is upstream of the app and is
    // the one server-side failure the vet can act on, so the screen has to be able to say so
    // rather than print "something went wrong".
    let no_mail_server = app
        .post(
            &format!("/api/invoices/{invoice_id}/send"),
            json!({ "recipient_emails": ["andere@example.com"] }),
        )
        .await;
    assert_eq!(no_mail_server.status, StatusCode::BAD_GATEWAY);
    assert_eq!(no_mail_server.json()["detail"], "invoice.mailUnreachable");

    let after = app.get(&format!("/api/invoices/{invoice_id}")).await.json();
    assert_eq!(after["status"], "accepted");
    assert!(after["ts_sent_email"].is_null());
    assert_eq!(
        after["email_recipients"][0], "andere@example.com",
        "the address the vet just typed is the one a retry uses"
    );
}

#[sqlx::test]
async fn a_postal_hand_over_is_recorded_by_hand(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let billed = billable_treatment(&app, &pool).await;
    let invoice = create_invoice(&app, billed.treatment_id).await;
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();

    let too_early = app
        .post_empty(&format!("/api/invoices/{invoice_id}/mark-posted"))
        .await;
    assert_eq!(
        too_early.status,
        StatusCode::CONFLICT,
        "an invoice that was never released cannot have been posted"
    );

    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;

    let posted = app
        .post_empty(&format!("/api/invoices/{invoice_id}/mark-posted"))
        .await;
    assert_eq!(
        posted.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&posted.body)
    );
    let posted = posted.json();
    assert_eq!(posted["status"], "sent");
    assert!(!posted["ts_sent_post"].is_null(), "FR-035 wants the stamp");
    assert!(
        posted["ts_sent_email"].is_null(),
        "nothing was emailed — the two routes stay distinguishable"
    );

    // Only the accepted stage offers the button, so saying it twice is a mistake.
    let again = app
        .post_empty(&format!("/api/invoices/{invoice_id}/mark-posted"))
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

/// The regression guard for the bug migration `0009` fixed.
///
/// GOT fees are published **net**, and `0008` imported them as such — but everything downstream
/// used to treat them as gross and extract VAT from them, billing every position about 16 % too
/// low. The expected values here were not produced by this project: they are read off the
/// practice's own RE-289, written by the system k-vet replaces. See item `GOT-01` in `review.md`.
#[sqlx::test]
async fn got_positions_are_billed_with_vat_on_top(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // (GOT number, published net fee, gross amount on RE-289)
    let positions = [
        ("16", "23.62", "28.11"),
        ("17", "15.39", "18.31"),
        ("40", "34.50", "41.06"),
        ("251", "17.25", "20.53"),
        ("394", "16.50", "19.64"),
        ("662", "10.26", "12.21"),
    ];

    for (got_number, net, gross) in positions {
        let customer_id = common::seed_customer(&pool).await;
        let patient_id = common::seed_patient(&pool, customer_id).await;
        let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
        let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

        // Straight from the catalogue migration `0008` imported, not from a fixture.
        let service_id: i64 =
            sqlx::query_scalar("SELECT id FROM service WHERE got_number = $1 AND type = 'got'")
                .bind(got_number)
                .fetch_one(&pool)
                .await
                .expect("the GOT catalogue is in every test database");

        let added = app
            .post(
                &format!("/api/treatments/{treatment_id}/items"),
                json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
            )
            .await;
        assert_eq!(
            added.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&added.body)
        );

        let item = added.json();
        assert_eq!(
            item["price_net"], net,
            "GOT {got_number} keeps the published net fee",
        );
        assert_eq!(
            item["line_gross"], gross,
            "GOT {got_number} must bill {gross} EUR — the amount on RE-289 — not the net fee",
        );

        let invoice = create_invoice(&app, treatment_id).await;
        assert_eq!(
            invoice["total_gross"], gross,
            "GOT {got_number}: the invoice total is the gross amount",
        );
    }
}

/// Renders a two-animal invoice with the practice fully configured and writes it to
/// `KVET_PDF_DUMP` when set, so the layout can be eyeballed against RE-289.
#[sqlx::test]
async fn a_realistic_invoice_renders_for_inspection(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    sqlx::query(
        "UPDATE global_settings SET
             practice_name = 'Mobile Tierärztin Dr. Erika Musterfrau',
             practice_street = 'Musterstraße 1', practice_zip = '12345',
             practice_city = 'Musterstadt', practice_country = 'DE',
             email = 'praxis@example.com', iban = 'DE02120300000000202051',
             bic = 'BYLADEM1001', bank_name = 'Musterbank', ustid = 'DE123456789'
         WHERE id",
    )
    .execute(&pool)
    .await
    .expect("settings");

    let customer_id = common::seed_customer(&pool).await;
    sqlx::query(
        "UPDATE customer SET salutation = 'herr', first_name = 'Thomas',
             last_name = 'Mustermann', company = 'Hundepension Musterhof GmbH',
             home_street = 'Musterweg 7a',
             home_zip = '12345', home_city = 'Musterstadt' WHERE id = $1",
    )
    .bind(customer_id)
    .execute(&pool)
    .await
    .expect("customer");

    let eddie = common::seed_patient(&pool, customer_id).await;
    let helene = common::seed_patient(&pool, customer_id).await;
    sqlx::query(
        "UPDATE patient SET name = 'Eddie', species = 'Hund', race = 'Havaneser',
             date_of_birth = DATE '2021-01-01' WHERE id = $1",
    )
    .bind(eddie)
    .execute(&pool)
    .await
    .expect("eddie");
    sqlx::query(
        "UPDATE patient SET name = 'Helene', species = 'Heimtier',
             race = 'Kaninchen (Zwergwidder)', date_of_birth = DATE '2017-07-01' WHERE id = $1",
    )
    .bind(helene)
    .execute(&pool)
    .await
    .expect("helene");

    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, eddies_record) =
        common::seed_patient_treatment(&pool, appointment_id, eddie).await;
    let helenes_record: i64 = sqlx::query_scalar(
        "INSERT INTO patient_treatment (treatment_id, patient_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(treatment_id)
    .bind(helene)
    .fetch_one(&pool)
    .await
    .expect("second patient");

    // A reason and a finding per animal — the invoice prints one block for each.
    app.patch(
        &format!("/api/patient-treatments/{eddies_record}"),
        json!({
            "treatment_reason": "Vorstellung zum Verbandswechsel.",
            "finding": "Allgemeinbefinden: gut. Wunde sauber und trocken.",
        }),
    )
    .await;
    app.patch(
        &format!("/api/patient-treatments/{helenes_record}"),
        json!({
            "treatment_reason": "Ohrenkontrolle.",
            "finding": "Otitis externa beidseitig, behandelt.",
        }),
    )
    .await;

    for (got_number, record_id) in [
        ("16", eddies_record),
        ("251", eddies_record),
        ("17", helenes_record),
    ] {
        let service_id: i64 =
            sqlx::query_scalar("SELECT id FROM service WHERE got_number = $1 AND type = 'got'")
                .bind(got_number)
                .fetch_one(&pool)
                .await
                .expect("GOT position");
        app.post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "service", "service_id": service_id,
                "quantity": "1", "patient_treatment_id": record_id,
            }),
        )
        .await;
    }

    let invoice = create_invoice(&app, treatment_id).await;
    let pdf = app
        .get(&format!("/api/invoices/{}/pdf", invoice["id"]))
        .await;
    assert_eq!(pdf.status, StatusCode::OK);
    assert!(pdf.body.starts_with(b"%PDF-"));

    if let Ok(path) = std::env::var("KVET_PDF_DUMP") {
        std::fs::write(&path, &pdf.body).expect("write the PDF");
        println!("wrote {} bytes to {path}", pdf.body.len());
    }
}
