//! Treatment lines: price pinning, FEFO dispenses, patient attribution, reordering (T026).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid test date")
}

/// A customer with one patient, an appointment and a treatment.
async fn treatment(pool: &PgPool) -> (i64, i64) {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(pool, appointment_id, patient_id).await;
    (treatment_id, patient_id)
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
async fn insufficient_stock_still_books_the_dispense(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let (treatment_id, _) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("10"), None).await;

    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "3"
        }),
    )
    .await;

    // The shelf is the truth: the shortfall is booked and the lot goes negative.
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("-20"));
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
    let (treatment_id, patient_id) = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    let drug_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "1" }),
        )
        .await
        .json();
    assert_eq!(
        drug_line["patient_id"], patient_id,
        "single patient is preselected"
    );

    let service_line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await
        .json();
    assert_eq!(service_line["patient_id"], patient_id);

    // Clearing the patient of a drug line is rejected.
    let cleared = app
        .patch(
            &format!(
                "/api/treatment-items/{}",
                drug_line["id"].as_i64().unwrap_or_default()
            ),
            json!({ "patient_id": null }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(cleared.error_fields().contains(&"patient_id".to_owned()));
}

#[sqlx::test]
async fn a_drug_line_needs_an_explicit_patient_when_several_are_treated(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let first = common::seed_patient(&pool, customer_id).await;
    let second = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, first).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/patients"),
        json!({ "patient_id": second }),
    )
    .await;
    let drug = common::seed_drug(&pool).await;

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
                "patient_id": second
            }),
        )
        .await;
    assert_eq!(with_patient.status, StatusCode::OK);
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
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

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
    let (treatment_id, _) = treatment(&pool).await;

    let patched = app
        .patch(
            &format!("/api/treatments/{treatment_id}"),
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
            &format!("/api/treatments/{treatment_id}"),
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
