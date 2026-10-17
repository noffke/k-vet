//! Appointments: draft rules, auto-save PATCH, duplication, delete guard (T030), and the one
//! treatment each of them has (migration 0022).

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
async fn the_list_shows_newest_first_each_with_its_treatment(pool: PgPool) {
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
    let newer_treatment = common::seed_treatment(&pool, newer, patient_id).await;

    let list = app.get("/api/appointments").await.json();

    assert_eq!(list[0]["id"], newer);
    assert_eq!(list[0]["treatment_id"], newer_treatment);
    assert_eq!(list[1]["id"], older);
    assert!(
        list[1]["treatment_id"].as_i64().is_some(),
        "an appointment without animals has its treatment all the same"
    );
}

#[sqlx::test]
async fn the_list_marks_appointments_whose_work_nobody_billed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let appointment_id = common::seed_appointment(
        &pool,
        "2026-05-04T09:00:00Z".parse::<DateTime<Utc>>().unwrap(),
    )
    .await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    // A treatment without positions is nothing to bill, so it is nothing to mark.
    let list = app.get("/api/appointments").await.json();
    assert_eq!(list[0]["unbilled"], false);

    let service_id = common::seed_got_service(&pool).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    let list = app.get("/api/appointments").await.json();
    assert_eq!(list[0]["unbilled"], true);

    // The invoice is what clears it — writing one is enough, it need not be released.
    app.post(
        &format!("/api/treatments/{treatment_id}/invoice"),
        json!({}),
    )
    .await;
    let list = app.get("/api/appointments").await.json();
    assert_eq!(list[0]["unbilled"], false);
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
    assert_ne!(
        copy["treatment_id"], treatment_id,
        "the copy has its own treatment"
    );
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

    let copied = app
        .get(&format!("/api/treatments/{}", copy["treatment_id"]))
        .await
        .json();
    let items = app
        .get(&format!("/api/treatments/{}/items", copy["treatment_id"]))
        .await
        .json();
    assert_eq!(items[0]["price_net"], "23.62", "prices are copied verbatim");
    assert_eq!(copied["patients"][0]["patient_id"], patient_id);
    assert_eq!(
        items[0]["patient_treatment_id"], copied["patients"][0]["id"],
        "the copied line belongs to the copy's own record for that animal, not the source's"
    );
    // The copy used to leave the customer behind; the keys skip NULLs, so that was accepted —
    // and gave a treatment that could never be billed.
    assert_eq!(
        copied["customer_id"], customer_id,
        "the customer comes along"
    );
    let invoice = app
        .post(
            &format!("/api/treatments/{}/invoice", copy["treatment_id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        invoice.status,
        StatusCode::OK,
        "the copy can be billed: {}",
        String::from_utf8_lossy(&invoice.body)
    );
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

#[sqlx::test]
async fn creating_an_appointment_creates_its_one_treatment(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    let created = app.post("/api/appointments", json!({})).await.json();
    let treatment = app
        .get(&format!("/api/treatments/{}", created["treatment_id"]))
        .await
        .json();

    assert_eq!(treatment["appointment_id"], created["id"]);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM treatment WHERE appointment_id = $1")
        .bind(created["id"].as_i64())
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 1);
}

/// A second invoice for the same visit is a second appointment; the database refuses a second
/// treatment from any code path.
#[sqlx::test]
async fn a_second_treatment_on_one_appointment_is_impossible(pool: PgPool) {
    let id = common::seed_appointment(&pool, Utc::now()).await;

    let second = sqlx::query("INSERT INTO treatment (appointment_id) VALUES ($1)")
        .bind(id)
        .execute(&pool)
        .await;

    assert!(second.is_err(), "one treatment per appointment");
}

/// The animals belong to the customer, so the customer can change until the first one is on
/// the treatment — and the treatment follows every change until then.
#[sqlx::test]
async fn the_customer_can_change_until_an_animal_is_attached(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let first = common::seed_customer(&pool).await;
    let second = common::seed_customer(&pool).await;
    let animal = common::seed_patient(&pool, second).await;
    let appointment = app
        .post(
            "/api/appointments",
            json!({ "starts_at": "2026-05-04T09:30:00Z" }),
        )
        .await
        .json();
    let uri = format!("/api/appointments/{}", appointment["id"]);
    let treatment_uri = format!("/api/treatments/{}", appointment["treatment_id"]);

    app.patch(&uri, json!({ "customer_id": first })).await;
    assert_eq!(app.get(&treatment_uri).await.json()["customer_id"], first);
    app.patch(&uri, json!({ "customer_id": second })).await;
    assert_eq!(app.get(&treatment_uri).await.json()["customer_id"], second);

    app.post(
        &format!("{treatment_uri}/patients"),
        json!({ "patient_id": animal }),
    )
    .await;
    let locked = app.patch(&uri, json!({ "customer_id": first })).await;
    assert_eq!(locked.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(locked.error_fields().contains(&"customer_id".to_owned()));
    let cleared = app.patch(&uri, json!({ "customer_id": null })).await;
    assert_eq!(cleared.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.get(&uri).await.json()["customer_id"], second);
}

/// Migration 0022 on data written under the old rule: an appointment with two treatments is
/// split in two, one without a treatment gets its empty one, and moving an invoiced treatment
/// does not make its invoice PDF look stale.
#[sqlx::test(migrations = false)]
async fn the_migration_splits_and_backfills(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(21, &pool)
        .await
        .expect("migrate up to the rule");

    let customer_id = common::seed_customer(&pool).await;
    let shared: i64 = sqlx::query_scalar(
        "INSERT INTO appointment (starts_at, note, customer_id, draft)
         VALUES ('2026-03-02T10:00:00Z', 'Zwei Hunde', $1, false) RETURNING id",
    )
    .bind(customer_id)
    .fetch_one(&pool)
    .await
    .expect("appointment");
    let mut treatments = Vec::new();
    for _ in 0..2 {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO treatment (appointment_id, customer_id, updated_at)
             VALUES ($1, $2, '2026-03-02T11:00:00Z') RETURNING id",
        )
        .bind(shared)
        .bind(customer_id)
        .fetch_one(&pool)
        .await
        .expect("treatment");
        treatments.push(id);
    }
    let empty: i64 = sqlx::query_scalar(
        "INSERT INTO appointment (starts_at, draft) VALUES ('2026-03-03T10:00:00Z', false)
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("appointment without treatment");

    migrator.run_to(22, &pool).await.expect("apply 0022");

    let homes: Vec<(i64, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT appointment.id, appointment.note, appointment.customer_id
           FROM treatment JOIN appointment ON appointment.id = treatment.appointment_id
          WHERE treatment.id = ANY($1) ORDER BY treatment.id",
    )
    .bind(&treatments)
    .fetch_all(&pool)
    .await
    .expect("where the treatments are now");
    assert_eq!(homes[0].0, shared, "the oldest treatment stays");
    assert_ne!(
        homes[1].0, shared,
        "the second moves to a copy of the appointment"
    );
    assert_eq!(homes[1].1.as_deref(), Some("Zwei Hunde"));
    assert_eq!(homes[1].2, Some(customer_id));

    let untouched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM treatment
          WHERE id = ANY($1) AND updated_at = '2026-03-02T11:00:00Z'",
    )
    .bind(&treatments)
    .fetch_one(&pool)
    .await
    .expect("timestamps");
    assert_eq!(untouched, 2, "moving a treatment is not editing it");

    let backfilled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM treatment WHERE appointment_id = $1")
            .bind(empty)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(backfilled, 1, "every appointment has its treatment");
}

/// The list says whose visit it is and which animals came (issues.md, 2026-10-17), and can be
/// searched by both — the note is no longer a column.
#[sqlx::test]
async fn the_list_names_customer_and_animals_and_finds_them(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let appointment = common::seed_appointment(&pool, Utc::now()).await;
    let customer_id = common::seed_customer(&pool).await;
    let bello = common::seed_patient(&pool, customer_id).await;
    let minka = common::seed_patient(&pool, customer_id).await;
    sqlx::query("UPDATE patient SET name = 'Minka' WHERE id = $1")
        .bind(minka)
        .execute(&pool)
        .await
        .expect("rename");
    common::seed_treatment(&pool, appointment, bello).await;
    common::seed_treatment(&pool, appointment, minka).await;
    common::seed_appointment(&pool, Utc::now()).await;

    let list = app.get("/api/appointments").await.json();
    let row = list
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == appointment))
        .expect("the appointment is listed");
    assert_eq!(row["customer_name"], "Erika Mustermann");
    assert_eq!(row["patient_names"], json!(["Bello", "Minka"]));

    for term in ["minka", "Musterm"] {
        let found = app.get(&format!("/api/appointments?q={term}")).await.json();
        assert_eq!(
            found.as_array().map(Vec::len),
            Some(1),
            "searching {term:?} finds the one appointment"
        );
        assert_eq!(found[0]["id"], appointment);
    }
}
