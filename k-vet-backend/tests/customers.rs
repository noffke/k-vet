//! Customers: auto-save drafts, contact validation, archiving, search (T041).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn a_new_customer_is_an_empty_draft_that_completes_field_by_field(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post_empty("/api/customers").await;
    assert_eq!(created.status, StatusCode::OK);
    let id = created.id();
    assert_eq!(created.json()["draft"], true);
    assert_eq!(
        created.json()["missing_fields"],
        json!([
            "salutation",
            "last_name",
            "home_street",
            "home_zip",
            "home_city"
        ])
    );

    // Typing the name leaves the record incomplete but saved.
    let named = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "salutation": "frau", "first_name": "Erika", "last_name": "Mustermann" }),
        )
        .await
        .json();
    assert_eq!(named["draft"], true);
    assert_eq!(
        named["missing_fields"],
        json!(["home_street", "home_zip", "home_city"])
    );

    // The write that fills the last mandatory field completes the record — no save button.
    let complete = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "home_street": "Musterweg 5", "home_zip": "12345", "home_city": "Musterstadt" }),
        )
        .await
        .json();
    assert_eq!(complete["draft"], false);
    assert_eq!(complete["missing_fields"], json!([]));
}

#[sqlx::test]
async fn clearing_a_mandatory_field_is_rejected_and_the_last_value_stays(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let rejected = app
        .patch(&format!("/api/customers/{id}"), json!({ "last_name": "" }))
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"last_name".to_owned()));

    let unchanged = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(unchanged["last_name"], "Mustermann");
}

#[sqlx::test]
async fn phone_numbers_are_normalised_and_redisplayed_nationally(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let saved = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "phone": "0171 / 123 45 67" }),
        )
        .await
        .json();

    assert_eq!(saved["phone"], "+491711234567", "stored as E.164");
    assert!(
        saved["phone_display"]
            .as_str()
            .unwrap_or_default()
            .starts_with("0171"),
        "shown in the familiar national format: {}",
        saved["phone_display"]
    );
}

#[sqlx::test]
async fn an_invalid_phone_number_is_never_persisted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;
    app.patch(
        &format!("/api/customers/{id}"),
        json!({ "phone": "030 12345678" }),
    )
    .await;

    let rejected = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "phone": "keine Nummer" }),
        )
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"phone".to_owned()));

    let unchanged = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(
        unchanged["phone"], "+493012345678",
        "the last valid value is kept"
    );
}

#[sqlx::test]
async fn email_addresses_are_validated_and_typed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let added = app
        .post(
            &format!("/api/customers/{id}/emails"),
            json!({ "email": " erika@example.com ", "email_type": "work" }),
        )
        .await
        .json();
    assert_eq!(added["emails"][0]["email"], "erika@example.com", "trimmed");
    assert_eq!(added["emails"][0]["email_type"], "work");

    let rejected = app
        .post(
            &format!("/api/customers/{id}/emails"),
            json!({ "email": "erika(at)example.com" }),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"email".to_owned()));

    let email_id = added["emails"][0]["id"].as_i64().unwrap_or_default();
    let deleted = app
        .delete(&format!("/api/customer-emails/{email_id}"))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let after = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(after["emails"].as_array().map(Vec::len), Some(0));
}

#[sqlx::test]
async fn archived_customers_are_hidden_until_asked_for(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    app.post_empty(&format!("/api/customers/{id}/archive"))
        .await;

    let visible = app.get("/api/customers").await.json();
    assert_eq!(
        visible.as_array().map(Vec::len),
        Some(0),
        "hidden by default"
    );

    let with_archived = app.get("/api/customers?archived=true").await.json();
    assert_eq!(with_archived.as_array().map(Vec::len), Some(1));
    assert_eq!(with_archived[0]["archived"], true);

    app.post_empty(&format!("/api/customers/{id}/unarchive"))
        .await;
    let restored = app.get("/api/customers").await.json();
    assert_eq!(restored.as_array().map(Vec::len), Some(1));
}

#[sqlx::test]
async fn search_matches_both_names_of_a_household(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;
    app.patch(
        &format!("/api/customers/{id}"),
        json!({ "second_salutation": "herr", "second_last_name": "Schmidt" }),
    )
    .await;

    let by_first = app.get("/api/customers?q=Mustermann").await.json();
    assert_eq!(by_first.as_array().map(Vec::len), Some(1));

    let by_second = app.get("/api/customers?q=Schmidt").await.json();
    assert_eq!(
        by_second.as_array().map(Vec::len),
        Some(1),
        "the second name is searchable"
    );
    assert_eq!(by_second[0]["has_second_name"], true);

    let nothing = app.get("/api/customers?q=Meier").await.json();
    assert_eq!(nothing.as_array().map(Vec::len), Some(0));
}

/// issues.md 3: the vet looks a customer up by whatever they have in front of them — the street
/// on an envelope, the number on the phone display, or the animal's name.
#[sqlx::test]
async fn search_also_matches_street_phone_and_the_animals(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;
    common::seed_patient(&pool, id).await;
    app.patch(
        &format!("/api/customers/{id}"),
        json!({ "phone": "030 12345678" }),
    )
    .await;

    let by_street = app.get("/api/customers?q=Musterweg").await.json();
    assert_eq!(by_street.as_array().map(Vec::len), Some(1), "by street");

    let by_animal = app.get("/api/customers?q=Bello").await.json();
    assert_eq!(
        by_animal.as_array().map(Vec::len),
        Some(1),
        "by animal name"
    );

    // Stored as E.164 (`+493012345678`) but typed the way it is written down: the trunk zero
    // has to fall away for the two to meet.
    let national = app.get("/api/customers?q=030%201234").await.json();
    assert_eq!(
        national.as_array().map(Vec::len),
        Some(1),
        "national format"
    );

    let international = app.get("/api/customers?q=%2B4930123").await.json();
    assert_eq!(
        international.as_array().map(Vec::len),
        Some(1),
        "E.164 input"
    );

    // A term with no digits at all must not fall through the phone clause and match everyone.
    let nothing = app.get("/api/customers?q=Kaninchenweg").await.json();
    assert_eq!(nothing.as_array().map(Vec::len), Some(0));

    // A deceased animal is not among the names the row prints, so it must not surface its owner
    // either — a match the row cannot explain reads as a bug.
    let bello: i64 = sqlx::query_scalar("SELECT id FROM patient WHERE customer_id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("the seeded animal");
    app.patch(
        &format!("/api/patients/{bello}"),
        json!({ "date_of_death": "2026-05-03" }),
    )
    .await;
    let gone = app.get("/api/customers?q=Bello").await.json();
    assert_eq!(gone.as_array().map(Vec::len), Some(0));
}

/// issues.md 4: the list names a household by its animals, and deceased ones are not among them.
#[sqlx::test]
async fn the_customer_carries_the_names_of_its_living_animals(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;
    let bello = common::seed_patient(&pool, id).await;
    let minka = common::seed_patient(&pool, id).await;
    app.patch(
        &format!("/api/patients/{minka}"),
        json!({ "name": "Minka" }),
    )
    .await;

    let customer = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(customer["patient_names"], json!(["Bello", "Minka"]));

    // A date of death archives the animal; either way it drops off the customer.
    app.patch(
        &format!("/api/patients/{bello}"),
        json!({ "date_of_death": "2026-05-03" }),
    )
    .await;
    let after = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(after["patient_names"], json!(["Minka"]));
}

/// issues.md 20: an invoice needs more than the record does — a first name and an address —
/// but that is reported, never enforced. The `customer_complete` CHECK stays where it is.
#[sqlx::test]
async fn a_customer_reports_what_an_invoice_would_still_be_missing(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let complete = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(complete["invoice_missing_fields"], json!([]));

    // A household recorded as "Familie Mustermann" is a perfectly valid customer ...
    let without_first_name = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "first_name": null }),
        )
        .await;
    assert_eq!(without_first_name.status, StatusCode::OK);
    let record = without_first_name.json();
    assert_eq!(
        record["draft"], false,
        "the record itself is still complete"
    );
    assert_eq!(record["missing_fields"], json!([]));
    // ... but not one to send an invoice to.
    assert_eq!(record["invoice_missing_fields"], json!(["first_name"]));
}

/// The set follows the address the invoice would actually print, so a complete separate
/// invoice address is not judged by gaps in the home address nobody will see.
#[sqlx::test]
async fn invoice_readiness_follows_the_address_that_would_be_printed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;
    app.patch(
        &format!("/api/customers/{id}"),
        json!({ "first_name": null }),
    )
    .await;

    let record = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({
                "invoice_salutation": "herr",
                "invoice_first_name": "Karl",
                "invoice_last_name": "Mustermann",
                "invoice_street": "Rechnungsweg 1",
                "invoice_zip": "54321",
                "invoice_city": "Zahlstadt",
            }),
        )
        .await
        .json();

    assert_eq!(record["has_invoice_address"], true);
    assert_eq!(
        record["invoice_missing_fields"],
        json!([]),
        "the home address's missing first name is not what gets printed"
    );
}

/// issues.md, follow-up: the country field is prefilled rather than left to a render-time
/// fallback, so what the vet sees on the form is what is stored.
#[sqlx::test]
async fn a_new_customer_starts_with_the_configured_country(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let created = app.post_empty("/api/customers").await.json();
    assert_eq!(created["home_country"], "DE");
    // Still a draft — a prefilled country is not one of the mandatory fields.
    assert_eq!(created["draft"], true);
}

#[sqlx::test]
async fn an_invoice_address_is_only_used_once_it_is_complete(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    // Auto-save stores the group field by field ...
    let partial = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "invoice_street": "Rechnungsallee 9" }),
        )
        .await;
    assert_eq!(
        partial.status,
        StatusCode::OK,
        "a half-typed group must be storable"
    );
    assert_eq!(
        partial.json()["has_invoice_address"],
        false,
        "... but it does not replace the home address yet"
    );

    let complete = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({
                "invoice_salutation": "familie",
                "invoice_last_name": "Mustermann",
                "invoice_zip": "54321",
                "invoice_city": "Zahlstadt"
            }),
        )
        .await
        .json();
    assert_eq!(complete["has_invoice_address"], true);
}

#[sqlx::test]
async fn a_company_round_trips_per_address_and_stays_optional(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let saved = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({
                "company": "Hundepension Musterhof GmbH",
                "invoice_company": "Musterhof Verwaltungs KG"
            }),
        )
        .await
        .json();
    assert_eq!(saved["company"], "Hundepension Musterhof GmbH");
    assert_eq!(saved["invoice_company"], "Musterhof Verwaltungs KG");
    assert!(
        !saved["missing_fields"]
            .as_array()
            .expect("missing_fields")
            .iter()
            .any(|field| field == "company"),
        "a company is optional — it must never hold a customer in draft",
    );

    // Cleared again: an empty string means "no company", not a blank first address line.
    let cleared = app
        .patch(&format!("/api/customers/{id}"), json!({ "company": "" }))
        .await
        .json();
    assert!(cleared["company"].is_null());
    assert_eq!(
        cleared["invoice_company"], "Musterhof Verwaltungs KG",
        "clearing one address's company leaves the other alone",
    );
}

#[sqlx::test]
async fn a_warning_remark_round_trips(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let id = common::seed_customer(&pool).await;

    let saved = app
        .patch(
            &format!("/api/customers/{id}"),
            json!({ "warning_remark": "Hund beißt bei der Blutabnahme" }),
        )
        .await
        .json();

    assert_eq!(saved["warning_remark"], "Hund beißt bei der Blutabnahme");
}
