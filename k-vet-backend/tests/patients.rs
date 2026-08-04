//! Patients: draft completion, death-date archiving, files, search (T041).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn a_patient_completes_when_owner_name_sex_and_species_are_known(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;

    let created = app
        .post("/api/patients", json!({ "customer_id": customer_id }))
        .await;
    let id = created.id();
    assert_eq!(created.json()["draft"], true);
    assert_eq!(
        created.json()["missing_fields"],
        json!(["name", "sex", "species"])
    );

    let complete = app
        .patch(
            &format!("/api/patients/{id}"),
            json!({ "name": "Bello", "sex": "male", "species": "Hund" }),
        )
        .await
        .json();

    assert_eq!(complete["draft"], false);
    assert_eq!(complete["customer_name"], "Erika Mustermann");
}

#[sqlx::test]
async fn an_unknown_sex_value_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let id = common::seed_patient(&pool, customer_id).await;

    let rejected = app
        .patch(&format!("/api/patients/{id}"), json!({ "sex": "diverse" }))
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"sex".to_owned()));
}

#[sqlx::test]
async fn a_date_of_death_archives_the_patient(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let id = common::seed_patient(&pool, customer_id).await;

    let deceased = app
        .patch(
            &format!("/api/patients/{id}"),
            json!({ "date_of_death": "2026-05-04" }),
        )
        .await
        .json();

    assert_eq!(deceased["date_of_death"], "2026-05-04");
    assert_eq!(
        deceased["archived"], true,
        "setting the date of death archives the record"
    );

    // Un-archiving a deceased patient would be a data error, not a restore.
    let refused = app
        .post_empty(&format!("/api/patients/{id}/unarchive"))
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn archived_patients_are_hidden_from_lists_by_default(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let id = common::seed_patient(&pool, customer_id).await;
    app.post_empty(&format!("/api/patients/{id}/archive")).await;

    assert_eq!(
        app.get("/api/patients")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        app.get("/api/patients?archived=true")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[sqlx::test]
async fn patients_can_be_filtered_by_owner_and_searched(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let first_customer = common::seed_customer(&pool).await;
    let second_customer = common::seed_customer(&pool).await;
    common::seed_patient(&pool, first_customer).await;
    let other = common::seed_patient(&pool, second_customer).await;
    app.patch(
        &format!("/api/patients/{other}"),
        json!({ "name": "Minka" }),
    )
    .await;

    let of_customer = app
        .get(&format!("/api/patients?customer_id={first_customer}"))
        .await
        .json();
    assert_eq!(of_customer.as_array().map(Vec::len), Some(1));
    assert_eq!(of_customer[0]["name"], "Bello");

    let by_name = app.get("/api/patients?q=Minka").await.json();
    assert_eq!(by_name.as_array().map(Vec::len), Some(1));

    let by_owner = app.get("/api/patients?q=Mustermann").await.json();
    assert_eq!(
        by_owner.as_array().map(Vec::len),
        Some(2),
        "the owner name is searchable"
    );
}

#[sqlx::test]
async fn customer_provided_files_carry_a_reference_date_and_note(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let uploaded = app
        .post_multipart(
            "/api/attachments",
            "vorbericht.pdf",
            "application/pdf",
            b"%PDF-1.7 lab report",
            &[
                ("kind", "patient_file"),
                ("patient_id", &patient_id.to_string()),
                ("reference_date", "2026-04-01"),
                ("note", "Vorbericht der Klinik"),
            ],
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::OK);

    let files = app
        .get(&format!("/api/patients/{patient_id}/files"))
        .await
        .json();
    assert_eq!(files.as_array().map(Vec::len), Some(1));
    assert_eq!(files[0]["reference_date"], "2026-04-01");
    assert_eq!(files[0]["note"], "Vorbericht der Klinik");

    let corrected = app
        .patch(
            &format!("/api/patient-files/{}", files[0]["id"]),
            json!({ "reference_date": "2026-03-15", "note": null }),
        )
        .await
        .json();
    assert_eq!(corrected["reference_date"], "2026-03-15");
    assert!(corrected["note"].is_null());
}

#[sqlx::test]
async fn a_photo_is_referenced_from_the_patient(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let mut png = Vec::new();
    let image = image::RgbImage::from_pixel(200, 200, image::Rgb([200, 150, 100]));
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encodes png");

    let uploaded = app
        .post_multipart(
            "/api/attachments",
            "bello.png",
            "image/png",
            &png,
            &[("kind", "referenced")],
        )
        .await;
    let attachment_id = uploaded.id();

    let patient = app
        .patch(
            &format!("/api/patients/{patient_id}"),
            json!({ "photo_attachment_id": attachment_id }),
        )
        .await
        .json();

    assert_eq!(patient["photo_attachment_id"], attachment_id);
    let thumbnail = app
        .get(&format!("/api/attachments/{attachment_id}/thumbnail"))
        .await;
    assert_eq!(
        thumbnail.status,
        StatusCode::OK,
        "the circle avatar has a thumbnail"
    );
}

/// The weight is optional, stored to one decimal, and must never hold a patient in draft.
#[sqlx::test]
async fn a_weight_round_trips_and_stays_optional(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let saved = app
        .patch(
            &format!("/api/patients/{patient_id}"),
            json!({ "weight_kg": "4.2" }),
        )
        .await
        .json();
    assert_eq!(saved["weight_kg"], "4.2");
    assert!(
        !saved["missing_fields"]
            .as_array()
            .expect("missing_fields")
            .iter()
            .any(|field| field == "weight_kg"),
        "a weight is optional — it must never hold a patient in draft",
    );
    assert_eq!(saved["draft"], false, "the patient stays complete");

    // Cleared again: not weighed is a legitimate state, not an empty string.
    let cleared = app
        .patch(
            &format!("/api/patients/{patient_id}"),
            json!({ "weight_kg": null }),
        )
        .await
        .json();
    assert!(cleared["weight_kg"].is_null());
    assert_eq!(cleared["draft"], false);
}

/// A second decimal is refused rather than silently rounded away by `NUMERIC(5,1)`.
#[sqlx::test]
async fn a_weight_keeps_at_most_one_decimal(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let rejected = app
        .patch(
            &format!("/api/patients/{patient_id}"),
            json!({ "weight_kg": "4.25" }),
        )
        .await;
    assert_eq!(
        rejected.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "4,25 kg must be reported, not quietly recorded as 4,3",
    );

    let unchanged = app.get(&format!("/api/patients/{patient_id}")).await.json();
    assert!(unchanged["weight_kg"].is_null(), "nothing was stored");
}

#[sqlx::test]
async fn a_weight_must_be_positive(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    for weight in ["0", "-1.5"] {
        let rejected = app
            .patch(
                &format!("/api/patients/{patient_id}"),
                json!({ "weight_kg": weight }),
            )
            .await;
        assert_eq!(
            rejected.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{weight} kg is not a weight",
        );
    }
}
