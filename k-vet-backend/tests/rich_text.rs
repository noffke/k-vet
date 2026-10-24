//! Formatted text in an animal's record: bold, italic, underline and one level of list in
//! "Vorbericht und Untersuchung" and "Therapie und weiteres Vorgehen" (migration `0030`).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

/// What the editor sends for a reason with a bold word and a numbered list — including the
/// attributes TipTap adds that mean nothing here.
fn written() -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "attrs": { "textAlign": null }, "content": [
            { "type": "text", "text": "Ohr " },
            { "type": "text", "text": "gerötet", "marks": [{ "type": "bold" }] },
        ] },
        { "type": "orderedList", "attrs": { "start": 1, "type": null }, "content": [
            { "type": "listItem", "content": [{ "type": "paragraph", "content": [
                { "type": "text", "text": "spülen" },
            ] }] },
        ] },
    ] })
}

/// The same document as it is stored and read back.
fn stored() -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [
            { "type": "text", "text": "Ohr " },
            { "type": "text", "text": "gerötet", "marks": [{ "type": "bold" }] },
        ] },
        { "type": "orderedList", "attrs": { "start": 1 }, "content": [
            { "type": "listItem", "content": [{ "type": "paragraph", "content": [
                { "type": "text", "text": "spülen" },
            ] }] },
        ] },
    ] })
}

fn blank() -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph" },
        { "type": "paragraph", "content": [{ "type": "text", "text": "  " }] },
    ] })
}

#[sqlx::test]
async fn the_record_keeps_its_formatting(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, record_id) =
        common::seed_patient_treatment(&pool, appointment_id, patient_id).await;
    let uri = format!("/api/patient-treatments/{record_id}");

    let patched = app
        .patch(&uri, json!({ "treatment_reason": written() }))
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(patched.json()["treatment_reason"], stored());
    assert_eq!(app.get(&uri).await.json()["treatment_reason"], stored());
    let treatment = app
        .get(&format!("/api/treatments/{treatment_id}"))
        .await
        .json();
    assert_eq!(treatment["patients"][0]["treatment_reason"], stored());

    // The patient's visit history shows it on one line, as words.
    let visits = app
        .get(&format!("/api/patients/{patient_id}/treatments"))
        .await
        .json();
    assert_eq!(visits[0]["treatment_reason"], "Ohr gerötet\n1. spülen");
}

/// Emptying the editor sends a document of empty paragraphs. It is stored as no text at all —
/// otherwise an emptied field would still count as something entered and lock the customer.
#[sqlx::test]
async fn an_emptied_text_is_null_and_locks_nothing(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let first = common::seed_customer(&pool).await;
    let second = common::seed_customer(&pool).await;
    common::seed_patient(&pool, first).await;
    common::seed_patient(&pool, second).await;
    let appointment = app
        .post(
            "/api/appointments",
            json!({ "starts_at": "2026-05-04T09:30:00Z", "customer_id": first }),
        )
        .await
        .json();
    let uri = format!("/api/appointments/{}", appointment["id"]);
    let treatment_uri = format!("/api/treatments/{}", appointment["treatment_id"]);
    let record =
        |treatment: &Value| format!("/api/patient-treatments/{}", treatment["patients"][0]["id"]);

    let treatment = app.get(&treatment_uri).await.json();
    let emptied = app
        .patch(
            &record(&treatment),
            json!({ "treatment_reason": blank(), "finding": blank() }),
        )
        .await
        .json();
    assert_eq!(emptied["treatment_reason"], Value::Null);
    assert_eq!(emptied["finding"], Value::Null);

    let moved = app.patch(&uri, json!({ "customer_id": second })).await;
    assert_eq!(moved.status, StatusCode::OK, "nothing was entered yet");

    // Real text is something entered.
    let treatment = app.get(&treatment_uri).await.json();
    app.patch(&record(&treatment), json!({ "finding": written() }))
        .await;
    let locked = app.patch(&uri, json!({ "customer_id": first })).await;
    assert_eq!(locked.status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// Only what the editor offers is stored — a heading or a list inside a list would be a
/// document the editor could not show and the invoice would not print.
#[sqlx::test]
async fn a_text_the_editor_cannot_show_is_refused(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (_, record_id) = common::seed_patient_treatment(&pool, appointment_id, patient_id).await;
    let uri = format!("/api/patient-treatments/{record_id}");

    let heading = json!({ "type": "doc", "content": [
        { "type": "heading", "attrs": { "level": 1 }, "content": [{ "type": "text", "text": "x" }] },
    ] });
    let nested = json!({ "type": "doc", "content": [{ "type": "bulletList", "content": [
        { "type": "listItem", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "a" }] },
            { "type": "bulletList", "content": [{ "type": "listItem", "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "b" }] },
            ] }] },
        ] },
    ] }] });
    for refused in [heading, nested, json!("Routinekontrolle")] {
        let response = app
            .patch(&uri, json!({ "treatment_reason": refused }))
            .await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{refused} is not a document the editor writes"
        );
    }
    assert_eq!(
        app.get(&uri).await.json()["treatment_reason"],
        common::doc("Routinekontrolle"),
        "a refused text changes nothing"
    );
}

/// Text written before formatting existed became a document of one paragraph per line; the
/// function that did it stays, for anything that only has plain text to write.
#[sqlx::test]
async fn plain_text_becomes_a_document_line_by_line(pool: PgPool) {
    let convert = |plain: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<Value>>("SELECT rich_text_from_plain($1)")
                .bind(plain)
                .fetch_one(&pool)
                .await
                .expect("convert")
        }
    };

    assert_eq!(
        convert("Ohr gerötet\r\n\nlinks *stärker*").await,
        Some(common::doc("Ohr gerötet\n\nlinks *stärker*")),
        "a blank line inside stays, and nothing becomes formatting"
    );
    assert_eq!(
        convert("\n \nRuhe\n\n").await,
        Some(common::doc("Ruhe")),
        "blank lines around the text go"
    );
    assert_eq!(convert("  \n ").await, None, "no text is no document");

    let plain: String = sqlx::query_scalar("SELECT rich_text_plain($1)")
        .bind(stored())
        .fetch_one(&pool)
        .await
        .expect("plain text");
    assert_eq!(plain, "Ohr gerötet\nspülen");
}

/// The database holds the rule too: a column of formatted text is a document with text in it,
/// or NULL.
#[sqlx::test]
async fn the_database_refuses_a_document_without_text(pool: PgPool) {
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (_, record_id) = common::seed_patient_treatment(&pool, appointment_id, patient_id).await;

    for refused in [
        blank(),
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": "x" }] }),
    ] {
        let written = sqlx::query("UPDATE patient_treatment SET finding = $2 WHERE id = $1")
            .bind(record_id)
            .bind(&refused)
            .execute(&pool)
            .await;
        assert!(written.is_err(), "{refused} is refused");
    }
    let written = sqlx::query("UPDATE patient_treatment SET finding = $2 WHERE id = $1")
        .bind(record_id)
        .bind(stored())
        .execute(&pool)
        .await;
    assert!(written.is_ok(), "a document with text is stored");
}
