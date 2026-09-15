//! Treatment lines: price pinning, FEFO dispenses, patient attribution, reordering (T026).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid test date")
}

/// A customer with one patient, an appointment and a treatment.
/// A treatment with one animal: its id, and the id of that animal's Patientenbehandlung —
/// which is what a position belongs to.
async fn treatment(pool: &PgPool) -> (i64, i64) {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    common::seed_patient_treatment(pool, appointment_id, patient_id).await
}

#[sqlx::test]
async fn line_values_are_pinned_at_entry_and_survive_catalog_changes(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

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
    assert_eq!(item["name"], "Allgemeine Untersuchung");
    assert_eq!(item["price_net"], "23.62");
    assert_eq!(item["vat_percent"], "19.000");
    assert_eq!(item["factor"], "100.000");
    assert_eq!(item["got_number"], "1");
    assert_eq!(item["line_net"], "23.62");

    // The catalog changes afterwards — the documented treatment must not move.
    sqlx::query("UPDATE service SET net_price = 99.99, name = 'Neuer Name' WHERE id = $1")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("catalog update");

    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(
        items[0]["price_net"], "23.62",
        "the pinned price is never re-read"
    );
    assert_eq!(items[0]["name"], "Allgemeine Untersuchung");
}

#[sqlx::test]
async fn line_total_applies_quantity_and_factor(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    let item = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "2" }),
        )
        .await
        .json();
    assert_eq!(item["line_net"], "47.24");

    // 23.62 × 2 × 1.5 = 70.86
    let patched = app
        .patch(
            &format!(
                "/api/treatment-items/{}",
                item["id"].as_i64().unwrap_or_default()
            ),
            json!({ "factor": "150.000" }),
        )
        .await
        .json();
    assert_eq!(patched["line_net"], "70.86");

    // issues.md 16: the unit price the invoice prints carries the Steigerungssatz, so
    // Einzelpreis × Menge reconciles with Gesamt instead of falling 50 % short.
    // 23.62 × 1.5 = 35.43 net → 42.16 gross at 19 %.
    assert_eq!(
        patched["price_net"], "23.62",
        "the editable net price is the fee itself"
    );
    assert_eq!(patched["price_gross"], "42.16");
}

/// The same figure reaches the picker, so the price previewed before a pick is the price the
/// line ends up costing (issues.md 16).
#[sqlx::test]
async fn the_picker_previews_the_price_the_line_will_have(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_got_service(&pool).await;
    sqlx::query("UPDATE service SET factor = 150.000 WHERE id = $1")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("raise the factor");

    let items = app.get("/api/picker/items?q=Allgemeine").await.json();
    let service = items
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["kind"] == "service" && entry["id"] == service_id)
        })
        .expect("the GOT position is offered");
    assert_eq!(service["price_net"], "23.62");
    assert_eq!(service["price_gross"], "42.16");
}

#[sqlx::test]
async fn drug_lines_dispense_fefo_and_split_across_lots(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;

    // Lot A expires first but holds only 20 ml; lot B is the fallback.
    let lot_a = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("20"),
        Some(date(2026, 6, 30)),
    )
    .await;
    let lot_b = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("100"),
        Some(date(2027, 1, 31)),
    )
    .await;

    // Three 10 ml subsets take 30 ml off the shelf.
    let item = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "3"
            }),
        )
        .await
        .json();

    let lots = item["lots"].as_array().expect("lots are reported").clone();
    assert_eq!(
        lots.len(),
        2,
        "the dispense split across two lots: {lots:?}"
    );
    assert_eq!(
        lots[0]["lot_id"], lot_a,
        "the lot expiring first is used first"
    );
    assert_eq!(lots[0]["quantity"], "20.00");
    assert_eq!(lots[1]["lot_id"], lot_b);
    assert_eq!(lots[1]["quantity"], "10.00");

    assert_eq!(common::lot_remaining(&pool, lot_a).await, dec("0"));
    assert_eq!(common::lot_remaining(&pool, lot_b).await, dec("90"));
}

#[sqlx::test]
async fn changing_the_quantity_recalculates_the_draft_dispense(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let item = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1"
            }),
        )
        .await
        .json();
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("90"));

    let item_id = item["id"].as_i64().unwrap_or_default();
    app.patch(
        &format!("/api/treatment-items/{item_id}"),
        json!({ "quantity": "3" }),
    )
    .await;
    assert_eq!(
        common::lot_remaining(&pool, lot).await,
        dec("70"),
        "draft dispenses are rewritten, not accumulated"
    );

    // One movement per lot, not one per edit.
    let movements: i64 =
        sqlx::query_scalar("SELECT count(*) FROM drug_stock_movement WHERE treatment_item_id = $1")
            .bind(item_id)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(movements, 1);
}

#[sqlx::test]
async fn a_dispense_the_stock_cannot_cover_is_refused(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("10"), None).await;

    // Three 10 ml subsets off a lot holding 10 ml.
    let refused = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "3"
            }),
        )
        .await;

    // Reversed deliberately (FR-018, issues.md 8): this used to assert the lot went to -20 on
    // the reasoning that the shelf is the truth. A negative remainder is not the shelf, it is
    // a record that the books were already wrong, and FEFO then keeps offering an empty lot.
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(refused.error_fields().contains(&"quantity".to_owned()));
    assert_eq!(
        common::lot_remaining(&pool, lot).await,
        dec("10"),
        "a refused line leaves the stock alone",
    );

    // What the lot can cover still goes through, so the vet is not blocked from dispensing.
    let allowed = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1"
            }),
        )
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
}

#[sqlx::test]
async fn the_lot_selection_can_be_overridden(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot_a = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("50"),
        Some(date(2026, 6, 30)),
    )
    .await;
    let lot_b = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("50"),
        Some(date(2027, 6, 30)),
    )
    .await;

    let item = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1"
            }),
        )
        .await
        .json();
    let item_id = item["id"].as_i64().unwrap_or_default();

    let overridden = app
        .post(
            &format!("/api/treatment-items/{item_id}/lots"),
            json!([{ "lot_id": lot_b, "quantity": "10" }]),
        )
        .await;
    assert_eq!(overridden.status, StatusCode::OK);

    assert_eq!(
        common::lot_remaining(&pool, lot_a).await,
        dec("50"),
        "FEFO choice released"
    );
    assert_eq!(common::lot_remaining(&pool, lot_b).await, dec("40"));

    // The selection has to add up to the line quantity.
    let mismatch = app
        .post(
            &format!("/api/treatment-items/{item_id}/lots"),
            json!([{ "lot_id": lot_b, "quantity": "3" }]),
        )
        .await;
    assert_eq!(mismatch.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn the_only_patient_is_preselected_and_drug_lines_require_one(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, record_id) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    // Stock to dispense from: a line the lots cannot cover is refused now (FR-018), and this
    // test is about which animal the line lands on.
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let service_id = common::seed_got_service(&pool).await;

    let drug_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "1" }),
        )
        .await
        .json();
    assert_eq!(
        drug_line["patient_treatment_id"], record_id,
        "the treatment's only animal is preselected"
    );

    let service_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await
        .json();
    assert_eq!(service_line["patient_treatment_id"], record_id);

    // Clearing the patient of a drug line is rejected.
    let cleared = app
        .patch(
            &format!(
                "/api/treatment-items/{}",
                drug_line["id"].as_i64().unwrap_or_default()
            ),
            json!({ "patient_treatment_id": null }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        cleared
            .error_fields()
            .contains(&"patient_treatment_id".to_owned())
    );
}

#[sqlx::test]
async fn a_drug_line_needs_an_explicit_patient_when_several_are_treated(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let first = common::seed_patient(&pool, customer_id).await;
    let second = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, _) = common::seed_patient_treatment(&pool, appointment_id, first).await;
    let attached = app
        .post(
            &format!("/api/treatments/{treatment_id}/patients"),
            json!({ "patient_id": second }),
        )
        .await
        .json();
    let seconds_record = attached["patients"]
        .as_array()
        .and_then(|patients| {
            patients
                .iter()
                .find(|patient| patient["patient_id"] == second)
        })
        .and_then(|patient| patient["id"].as_i64())
        .expect("the attached animal has a record");
    let drug = common::seed_drug(&pool).await;
    // Stock to dispense from — this test is about which animal a drug line belongs to, not
    // about what the lots can cover.
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let without_patient = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "1" }),
        )
        .await;
    assert_eq!(without_patient.status, StatusCode::UNPROCESSABLE_ENTITY);

    let with_patient = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.packaging_id,
                "quantity": "1",
                "patient_treatment_id": seconds_record
            }),
        )
        .await;
    assert_eq!(with_patient.status, StatusCode::OK);
}

#[sqlx::test]
async fn a_line_cannot_belong_to_another_treatments_animal(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let (_, foreign_record) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    let response = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "service", "service_id": service_id, "quantity": "1",
                "patient_treatment_id": foreign_record,
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response
            .error_fields()
            .contains(&"patient_treatment_id".to_owned())
    );
}

#[sqlx::test]
async fn positions_are_ordered_within_each_animal(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let first = common::seed_patient(&pool, customer_id).await;
    let second = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, firsts_record) =
        common::seed_patient_treatment(&pool, appointment_id, first).await;
    let seconds_record: i64 = sqlx::query_scalar(
        "INSERT INTO patient_treatment (treatment_id, patient_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(treatment_id)
    .bind(second)
    .fetch_one(&pool)
    .await
    .expect("second record");
    let service_id = common::seed_got_service(&pool).await;

    // Two lines for each animal, interleaved: each pair is numbered 1, 2 in its own group.
    let mut ids = Vec::new();
    for record_id in [firsts_record, seconds_record, firsts_record, seconds_record] {
        let line = app
            .post(
                &format!("/api/treatments/{treatment_id}/items"),
                json!({
                    "kind": "service", "service_id": service_id, "quantity": "1",
                    "patient_treatment_id": record_id,
                }),
            )
            .await
            .json();
        ids.push(line["id"].as_i64().unwrap_or_default());
        assert_eq!(
            line["position"],
            if ids.len() <= 2 { 1 } else { 2 },
            "positions start again for each animal"
        );
    }

    // Moving the second animal's last line up swaps it with the first line of *that* animal.
    app.post(
        &format!("/api/treatment-items/{}/move", ids[3]),
        json!({ "direction": "up" }),
    )
    .await;
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    let moved = items
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == ids[3]))
        .expect("the moved line");
    assert_eq!(moved["position"], 1);
    assert_eq!(
        moved["patient_treatment_id"], seconds_record,
        "moving inside a group does not move the line out of it"
    );
    let untouched = items
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == ids[0]))
        .expect("the first animal's first line");
    assert_eq!(
        untouched["position"], 1,
        "the other animal's order is its own"
    );
}

#[sqlx::test]
async fn a_line_moved_to_another_animal_lands_at_the_end(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let first = common::seed_patient(&pool, customer_id).await;
    let second = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, firsts_record) =
        common::seed_patient_treatment(&pool, appointment_id, first).await;
    let seconds_record: i64 = sqlx::query_scalar(
        "INSERT INTO patient_treatment (treatment_id, patient_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(treatment_id)
    .bind(second)
    .fetch_one(&pool)
    .await
    .expect("second record");
    let service_id = common::seed_got_service(&pool).await;

    let mut ids = Vec::new();
    for record_id in [firsts_record, firsts_record, seconds_record] {
        let line = app
            .post(
                &format!("/api/treatments/{treatment_id}/items"),
                json!({
                    "kind": "service", "service_id": service_id, "quantity": "1",
                    "patient_treatment_id": record_id,
                }),
            )
            .await
            .json();
        ids.push(line["id"].as_i64().unwrap_or_default());
    }

    // The first animal's first line moves across.
    let moved = app
        .patch(
            &format!("/api/treatment-items/{}", ids[0]),
            json!({ "patient_treatment_id": seconds_record }),
        )
        .await
        .json();
    assert_eq!(moved["patient_treatment_id"], seconds_record);
    assert_eq!(
        moved["position"], 2,
        "it lands behind what is already there"
    );

    // And the hole it left is closed, so the animal it came from is still 1..n.
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    let stayed = items
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == ids[1]))
        .expect("the line that stayed");
    assert_eq!(stayed["position"], 1);
}

#[sqlx::test]
async fn patients_of_a_treatment_must_share_one_customer(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;

    let other_customer = common::seed_customer(&pool).await;
    let other_patient = common::seed_patient(&pool, other_customer).await;

    let response = app
        .post(
            &format!("/api/treatments/{treatment_id}/patients"),
            json!({ "patient_id": other_patient }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.error_fields().contains(&"patient_id".to_owned()));
}

/// The same rule, asked of the database rather than the handler.
///
/// It lived only in `attach_patient` before, so it held exactly as long as every write went
/// through that function — and it engaged only once a first animal was attached, which is the
/// gap that let an empty treatment offer the whole practice. Composite keys close both.
#[sqlx::test]
async fn the_database_refuses_a_mixed_customer_treatment(pool: PgPool) {
    let (treatment_id, _) = treatment(&pool).await;
    let other_customer = common::seed_customer(&pool).await;
    let other_patient = common::seed_patient(&pool, other_customer).await;

    // Honest about the owner: caught by the key tying the record to its treatment's customer.
    let honest = sqlx::query(
        "INSERT INTO patient_treatment (treatment_id, patient_id, customer_id)
         VALUES ($1, $2, $3)",
    )
    .bind(treatment_id)
    .bind(other_patient)
    .bind(other_customer)
    .execute(&pool)
    .await;
    assert!(
        honest.is_err(),
        "an animal of another customer cannot be attached"
    );

    // Claiming this treatment's customer instead: caught by the key tying the record to the
    // animal's own owner. Between them there is no value that would be accepted.
    let treatment_customer: i64 =
        sqlx::query_scalar("SELECT customer_id FROM treatment WHERE id = $1")
            .bind(treatment_id)
            .fetch_one(&pool)
            .await
            .expect("the treatment has a customer");
    let dishonest = sqlx::query(
        "INSERT INTO patient_treatment (treatment_id, patient_id, customer_id)
         VALUES ($1, $2, $3)",
    )
    .bind(treatment_id)
    .bind(other_patient)
    .bind(treatment_customer)
    .execute(&pool)
    .await;
    assert!(dishonest.is_err(), "nor by mislabelling whose animal it is");
}

/// A treatment cannot be started until the appointment says whose visit it is — that answer is
/// what the animal picker is filtered by (issues.md 7).
#[sqlx::test]
async fn a_treatment_needs_the_appointments_customer(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;

    let refused = app
        .post(
            &format!("/api/appointments/{appointment_id}/treatments"),
            json!({ "patient_ids": [] }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(refused.error_fields().contains(&"customer_id".to_owned()));

    let customer_id = common::seed_customer(&pool).await;
    let named = app
        .patch(
            &format!("/api/appointments/{appointment_id}"),
            json!({ "customer_id": customer_id }),
        )
        .await;
    assert_eq!(named.status, StatusCode::OK);
    assert_eq!(named.json()["customer_id"], customer_id);

    let created = app
        .post(
            &format!("/api/appointments/{appointment_id}/treatments"),
            json!({ "patient_ids": [] }),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK);
    assert_eq!(
        created.json()["customer_id"],
        customer_id,
        "the treatment inherits it rather than waiting for an animal to imply it",
    );
}

#[sqlx::test]
async fn lines_can_be_reordered(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    let mut ids = Vec::new();
    for _ in 0..3 {
        let item = app
            .post(
                &format!("/api/treatments/{treatment_id}/items"),
                json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
            )
            .await
            .json();
        ids.push(item["id"].as_i64().unwrap_or_default());
    }
    let names = |items: &serde_json::Value| {
        items
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["id"].as_i64())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let third = ids[2];
    let moved = app
        .post(
            &format!("/api/treatment-items/{third}/move"),
            json!({ "direction": "top" }),
        )
        .await;
    assert_eq!(names(&moved.json()), vec![ids[2], ids[0], ids[1]]);

    let moved = app
        .post(
            &format!("/api/treatment-items/{third}/move"),
            json!({ "direction": "down" }),
        )
        .await;
    assert_eq!(names(&moved.json()), vec![ids[0], ids[2], ids[1]]);

    let moved = app
        .post(
            &format!("/api/treatment-items/{third}/move"),
            json!({ "direction": "bottom" }),
        )
        .await;
    assert_eq!(names(&moved.json()), vec![ids[0], ids[1], ids[2]]);

    // Moving the first line up is a no-op, not an error.
    let unmoved = app
        .post(
            &format!("/api/treatment-items/{}/move", ids[0]),
            json!({ "direction": "up" }),
        )
        .await;
    assert_eq!(unmoved.status, StatusCode::OK);
    assert_eq!(names(&unmoved.json()), vec![ids[0], ids[1], ids[2]]);
}

#[sqlx::test]
async fn deleting_a_line_removes_its_draft_dispense_and_closes_the_gap(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let service_id = common::seed_got_service(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let drug_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1"
            }),
        )
        .await
        .json();
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;

    let deleted = app
        .delete(&format!(
            "/api/treatment-items/{}",
            drug_line["id"].as_i64().unwrap_or_default()
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    assert_eq!(
        common::lot_remaining(&pool, lot).await,
        dec("100"),
        "stock is released"
    );
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(items.as_array().map(Vec::len), Some(1));
    assert_eq!(items[0]["position"], 1, "positions stay 1..n");
}

#[sqlx::test]
async fn applying_a_template_appends_its_items_in_order_with_current_prices(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let service_id = common::seed_got_service(&pool).await;
    // The template's drug line is five of the 100 ml original, so the lot has to hold 500 —
    // a line the stock cannot cover is refused now (FR-018) and this test is about ordering.
    common::seed_lot(&pool, drug.packaging_id, 5, dec("500"), None).await;

    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO treatment_template (name, draft) VALUES ('Impfung', false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed template");
    sqlx::query(
        "INSERT INTO treatment_template_item (template_id, position, kind, service_id, quantity)
         VALUES ($1, 1, 'service', $2, 1)",
    )
    .bind(template_id)
    .bind(service_id)
    .execute(&pool)
    .await
    .expect("seed template item");
    sqlx::query(
        "INSERT INTO treatment_template_item
             (template_id, position, kind, drug_packaging_id, quantity, unit)
         VALUES ($1, 2, 'drug_packaging', $2, 5, 'ml')",
    )
    .bind(template_id)
    .bind(drug.packaging_id)
    .execute(&pool)
    .await
    .expect("seed template item");

    // The catalog price at apply time is what gets pinned.
    sqlx::query("UPDATE service SET net_price = 30.00 WHERE id = $1")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("price change");

    let response = app
        .post(
            &format!("/api/treatments/{treatment_id}/apply-template"),
            json!({ "template_id": template_id }),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response.body)
    );

    let items = response.json();
    assert_eq!(items[0]["service_id"], service_id, "template order is kept");
    assert_eq!(items[0]["price_net"], "30.00", "current price is copied");
    assert_eq!(items[1]["drug_packaging_id"], drug.packaging_id);
    assert_eq!(items[1]["quantity"], "5.00");
    assert_eq!(
        items[1]["lots"].as_array().map(Vec::len),
        Some(1),
        "drug lines dispense"
    );
}

/// Positions are numbered per animal, so removing one animal's line must leave the other
/// animal's numbering alone — the treatment page shows one ordered group per animal and
/// `reorder` moves lines inside a group.
///
/// Closing the gap across the whole treatment instead does not merely renumber the other
/// animal: it walks its positions onto each other, and `UNIQUE (treatment_id,
/// patient_treatment_id, position)` then rejects the commit. Deleting anything but the last
/// line of a two-animal treatment used to fail with a 409 for exactly that reason.
#[sqlx::test]
async fn removing_a_line_renumbers_only_its_own_animal(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, first_record) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    // A second animal of the same customer, with its own list.
    let customer_id: i64 = sqlx::query_scalar(
        "SELECT patient.customer_id FROM patient_treatment record
         JOIN patient ON patient.id = record.patient_id WHERE record.id = $1",
    )
    .bind(first_record)
    .fetch_one(&pool)
    .await
    .expect("customer of the first animal");
    let second_patient = common::seed_patient(&pool, customer_id).await;
    let with_both = app
        .post(
            &format!("/api/treatments/{treatment_id}/patients"),
            json!({ "patient_id": second_patient }),
        )
        .await
        .json();
    let second_record = with_both["patients"]
        .as_array()
        .and_then(|patients| patients.iter().find(|p| p["patient_id"] == second_patient))
        .and_then(|patient| patient["id"].as_i64())
        .expect("the second animal's record");

    let mut first_lines = Vec::new();
    let mut second_lines = Vec::new();
    for record in [first_record, second_record] {
        for _ in 0..3 {
            let line = app
                .post(
                    &format!("/api/treatments/{treatment_id}/items"),
                    json!({ "kind": "service", "service_id": service_id, "quantity": "1",
                            "patient_treatment_id": record }),
                )
                .await
                .json();
            let id = line["id"].as_i64().unwrap_or_default();
            if record == first_record {
                first_lines.push(id);
            } else {
                second_lines.push(id);
            }
        }
    }

    let positions_of = |items: &Value, record: i64| -> Vec<i64> {
        items
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter(|row| row["patient_treatment_id"] == record)
                    .filter_map(|row| row["position"].as_i64())
                    .collect()
            })
            .unwrap_or_default()
    };

    // One line off the first animal: its own list closes up, the other is untouched.
    let removed = app
        .delete(&format!("/api/treatment-items/{}", first_lines[1]))
        .await;
    assert_eq!(
        removed.status,
        StatusCode::NO_CONTENT,
        "a middle line of a two-animal treatment comes out: {}",
        String::from_utf8_lossy(&removed.body)
    );
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(positions_of(&items, first_record), vec![1, 2]);
    assert_eq!(
        positions_of(&items, second_record),
        vec![1, 2, 3],
        "the other animal keeps its numbering: {items:?}"
    );

    // The same holds when several go at once.
    let remaining = app
        .post(
            &format!("/api/treatments/{treatment_id}/items/bulk-delete"),
            json!({ "item_ids": [second_lines[0], second_lines[2]] }),
        )
        .await;
    assert_eq!(
        remaining.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&remaining.body)
    );
    let remaining = remaining.json();
    assert_eq!(positions_of(&remaining, second_record), vec![1]);
    assert_eq!(positions_of(&remaining, first_record), vec![1, 2]);
}

#[sqlx::test]
async fn a_bulk_removal_refuses_lines_it_was_not_asked_about(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let (other_treatment_id, _) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    let elsewhere = app
        .post(
            &format!("/api/treatments/{other_treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await
        .json();
    let elsewhere_id = elsewhere["id"].as_i64().unwrap_or_default();

    // Nothing is removed when one id is wrong — the caller read them a moment ago, so a
    // stray id is a bug rather than something to shrug at.
    let rejected = app
        .post(
            &format!("/api/treatments/{treatment_id}/items/bulk-delete"),
            json!({ "item_ids": [elsewhere_id] }),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"item_ids".to_owned()));

    let untouched = app
        .get(&format!("/api/treatments/{other_treatment_id}/items"))
        .await
        .json();
    assert_eq!(untouched.as_array().map(Vec::len), Some(1));
}

#[sqlx::test]
async fn duplicating_a_treatment_copies_or_refreshes_prices(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let service_id = common::seed_got_service(&pool).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;

    sqlx::query("UPDATE service SET net_price = 40.00 WHERE id = $1")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("price change");

    let verbatim = app
        .post(
            &format!("/api/treatments/{treatment_id}/duplicate"),
            json!({ "price_mode": "verbatim" }),
        )
        .await
        .json();
    let verbatim_items = app
        .get(&format!(
            "/api/treatments/{}/items",
            verbatim["id"].as_i64().unwrap_or_default()
        ))
        .await
        .json();
    assert_eq!(
        verbatim_items[0]["price_net"], "23.62",
        "verbatim keeps the old price"
    );

    let refreshed = app
        .post(
            &format!("/api/treatments/{treatment_id}/duplicate"),
            json!({ "price_mode": "refresh" }),
        )
        .await
        .json();
    let refreshed_items = app
        .get(&format!(
            "/api/treatments/{}/items",
            refreshed["id"].as_i64().unwrap_or_default()
        ))
        .await
        .json();
    assert_eq!(
        refreshed_items[0]["price_net"], "40.00",
        "refresh re-reads the catalog"
    );
}

#[sqlx::test]
async fn treatment_text_is_patched_field_by_field(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    // The reason and the finding belong to the animal, not to the visit: two animals seen
    // together are rarely seen for the same thing.
    let (_, record_id) = treatment(&pool).await;

    let patched = app
        .patch(
            &format!("/api/patient-treatments/{record_id}"),
            json!({ "finding": "Ohne besonderen Befund" }),
        )
        .await
        .json();
    assert_eq!(patched["finding"], "Ohne besonderen Befund");
    assert_eq!(
        patched["treatment_reason"], "Routinekontrolle",
        "untouched fields stay"
    );

    // An explicit null clears the field; an absent field does not.
    let cleared = app
        .patch(
            &format!("/api/patient-treatments/{record_id}"),
            json!({ "finding": null }),
        )
        .await
        .json();
    assert!(cleared["finding"].is_null());
    assert_eq!(cleared["treatment_reason"], "Routinekontrolle");
}

#[sqlx::test]
async fn treatments_need_a_complete_appointment(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let draft_appointment = app.post("/api/appointments", json!({})).await;
    assert_eq!(draft_appointment.json()["draft"], true);

    let response = app
        .post(
            &format!("/api/appointments/{}/treatments", draft_appointment.id()),
            json!({}),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// The ad-hoc Umwidmung is a documentation flag on the line, and only a drug line can carry it.
#[sqlx::test]
async fn a_drug_line_can_be_redesignated_but_a_service_line_cannot(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, patient_id) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1",
                "patient_id": patient_id,
                "redesignation": true,
            }),
        )
        .await;
    assert_eq!(
        line.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&line.body)
    );
    let line = line.json();
    assert_eq!(line["redesignation"], true);
    let price_before = line["price_net"].clone();

    // Clearing it is just as ordinary, and the price never moves either way.
    let cleared = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "redesignation": false }),
        )
        .await
        .json();
    assert_eq!(cleared["redesignation"], false);
    assert_eq!(
        cleared["price_net"], price_before,
        "an Umwidmung documents, it does not price",
    );

    // A GOT position cannot be redesignated — the database refuses it.
    let service_id = common::seed_got_service(&pool).await;
    let service_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1",
                    "redesignation": true }),
        )
        .await;
    assert_ne!(
        service_line.status,
        StatusCode::OK,
        "only a dispensed drug can be redesignated"
    );
}
