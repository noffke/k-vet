//! Stock ledger: intake snapshots, derived reconciliation, append-only after invoicing,
//! corrections and batch traceability (T050).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

#[sqlx::test]
async fn an_intake_snapshots_packages_times_packaging_quantity(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;

    let lot = app
        .post(
            &format!("/api/packagings/{}/stock-intakes", drug.packaging_id),
            json!({
                "packages_received": 2,
                "batch_number": "CH-2026-01",
                "expiration_date": "2027-03-31"
            }),
        )
        .await;
    assert_eq!(
        lot.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&lot.body)
    );
    let lot = lot.json();

    // 2 packages × 100 ml.
    assert_eq!(lot["initial_quantity"], "200.00");
    assert_eq!(lot["remaining"], "200.00");
    assert_eq!(lot["packages_received"], 2);
    assert_eq!(lot["batch_number"], "CH-2026-01");
    assert!(
        !lot["arrival_date"].is_null(),
        "the arrival date defaults to today"
    );

    // Changing the packaging size afterwards must not rewrite what arrived.
    sqlx::query("UPDATE drug_packaging SET quantity = 250 WHERE id = $1")
        .bind(drug.packaging_id)
        .execute(&pool)
        .await
        .expect("packaging update");
    let unchanged = app.get(&format!("/api/lots/{}", lot["id"])).await.json();
    assert_eq!(unchanged["initial_quantity"], "200.00");
}

#[sqlx::test]
async fn stock_can_only_be_booked_on_original_packagings(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;

    let refused = app
        .post(
            &format!("/api/packagings/{}/stock-intakes", drug.subset_packaging_id),
            json!({ "packages_received": 1 }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn remaining_stock_reconciles_after_every_kind_of_movement(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    // Dispense 30 ml through a treatment (three 10 ml subsets).
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "3"
        }),
    )
    .await;
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("70"));

    // A stocktake finds 65 ml on the shelf.
    let corrected = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "65", "reason": "Inventur" }),
        )
        .await;
    assert_eq!(corrected.status, StatusCode::OK);
    assert_eq!(corrected.json()["remaining"], "65.00");
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("65"));

    // initial + Σ movements == remaining, always (SC-007).
    let sum: Decimal = sqlx::query_scalar(
        "SELECT initial_quantity + COALESCE((
             SELECT SUM(quantity) FROM drug_stock_movement WHERE lot_id = lot.id), 0)
         FROM drug_stock_lot lot WHERE id = $1",
    )
    .bind(lot)
    .fetch_one(&pool)
    .await
    .expect("derived sum");
    assert_eq!(sum, dec("65"));
}

#[sqlx::test]
async fn a_correction_records_the_difference_and_its_reason(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    // Breakage: 12 ml gone.
    let detail = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "88", "reason": "Bruch" }),
        )
        .await
        .json();

    let movements = detail["movements"].as_array().expect("movements").clone();
    assert_eq!(movements.len(), 1);
    assert_eq!(movements[0]["kind"], "correction");
    assert_eq!(
        movements[0]["quantity"], "-12.00",
        "the ledger stores the difference"
    );
    assert_eq!(movements[0]["reason"], "Bruch");

    // A correction that changes nothing is refused rather than written.
    let noop = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "88" }),
        )
        .await;
    assert_eq!(noop.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn a_correction_can_add_stock_again(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    app.post(
        &format!("/api/lots/{lot}/corrections"),
        json!({ "new_remaining": "120", "reason": "Nachlieferung nicht erfasst" }),
    )
    .await;

    assert_eq!(common::lot_remaining(&pool, lot).await, dec("120"));
}

#[sqlx::test]
async fn previously_used_reasons_are_offered_alphabetically(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    for (remaining, reason) in [("90", "Verfall"), ("80", "Bruch"), ("70", "Verfall")] {
        app.post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": remaining, "reason": reason }),
        )
        .await;
    }

    let reasons: Vec<String> = app.get("/api/stock/correction-reasons").await.parse();
    assert_eq!(reasons, vec!["Bruch", "Verfall"], "distinct and sorted");
}

#[sqlx::test]
async fn the_lot_history_links_every_dispense_to_its_treatment_and_customer(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "1"
        }),
    )
    .await;
    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await
        .json();
    let invoice_id = invoice["id"].as_i64().unwrap_or_default();
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;

    let detail = app.get(&format!("/api/lots/{lot}")).await.json();
    let dispense = &detail["movements"][0];

    // SC-004: from a batch number to the receiving customer in one view.
    assert_eq!(dispense["kind"], "dispense");
    assert_eq!(dispense["treatment_id"], treatment_id);
    assert_eq!(dispense["patient_id"], patient_id);
    assert_eq!(dispense["customer_id"], customer_id);
    assert_eq!(dispense["customer_name"], "Erika Mustermann");
    assert_eq!(dispense["invoice_id"], invoice_id);
    assert_eq!(dispense["invoice_number"], invoice["invoice_number"]);
    assert!(
        dispense["treatment_item_name"]
            .as_str()
            .unwrap_or_default()
            .contains("Amoxicillin"),
        "the dispensed line is named: {dispense:?}"
    );
}

#[sqlx::test]
async fn cancelling_an_invoice_appends_a_linked_counter_movement(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "2"
        }),
    )
    .await;
    let invoice_id = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await
        .id();
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;
    app.post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
        .await;

    let detail = app.get(&format!("/api/lots/{lot}")).await.json();
    let movements = detail["movements"].as_array().expect("movements").clone();

    assert_eq!(
        movements.len(),
        2,
        "the dispense is never deleted: {movements:?}"
    );
    assert_eq!(movements[0]["kind"], "dispense");
    assert_eq!(movements[1]["kind"], "correction");
    assert_eq!(
        movements[1]["reverses_movement_id"], movements[0]["id"],
        "the correction names the movement it reverses"
    );
    assert_eq!(detail["remaining"], "100.00", "the lot is whole again");
}

#[sqlx::test]
async fn lots_are_listed_fefo_and_used_up_ones_are_hidden(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let later = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("50"),
        chrono::NaiveDate::from_ymd_opt(2027, 6, 30),
    )
    .await;
    let sooner = common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        dec("50"),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 30),
    )
    .await;
    let empty = common::seed_lot(&pool, drug.packaging_id, 1, dec("10"), None).await;
    app.post(
        &format!("/api/lots/{empty}/corrections"),
        json!({ "new_remaining": "0", "reason": "aufgebraucht" }),
    )
    .await;

    let ids = |lots: &Value| {
        lots.as_array()
            .map(|lots| {
                lots.iter()
                    .filter_map(|lot| lot["id"].as_i64())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let with_stock = app
        .get(&format!("/api/lots?drug_id={}", drug.drug_id))
        .await
        .json();
    assert_eq!(
        ids(&with_stock),
        vec![sooner, later],
        "earliest expiration first, used-up lots hidden"
    );

    let all = app
        .get(&format!("/api/lots?drug_id={}&empty=true", drug.drug_id))
        .await
        .json();
    assert!(ids(&all).contains(&empty));
}

#[sqlx::test]
async fn frozen_movements_are_append_only(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    let item_id = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({
                "kind": "drug_packaging",
                "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "1"
            }),
        )
        .await
        .id();
    let invoice_id = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await
        .id();
    app.post(
        &format!("/api/invoices/{invoice_id}/accept"),
        json!({ "recipient_emails": [] }),
    )
    .await;

    // Neither editing nor deleting the line may touch a frozen movement.
    assert_eq!(
        app.patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "quantity": "5" })
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.delete(&format!("/api/treatment-items/{item_id}"))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("90"));
}

#[sqlx::test]
async fn the_movement_shape_checks_hold_at_the_database_level(pool: PgPool) {
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    // A dispense without a treatment line, or with a positive quantity, is not a dispense.
    for (quantity, item) in [("-5", None::<i64>), ("5", None)] {
        let result = sqlx::query(
            "INSERT INTO drug_stock_movement (lot_id, kind, quantity, treatment_item_id)
             VALUES ($1, 'dispense', $2::numeric, $3)",
        )
        .bind(lot)
        .bind(quantity)
        .bind(item)
        .execute(&pool)
        .await;
        assert!(
            result.is_err(),
            "a dispense of {quantity} without a line must be rejected"
        );
    }

    // A correction never points at a treatment line.
    let result = sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, treatment_item_id)
         VALUES ($1, 'correction', -1, 1)",
    )
    .bind(lot)
    .execute(&pool)
    .await;
    assert!(result.is_err());

    // A movement of zero says nothing.
    let result = sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity) VALUES ($1, 'correction', 0)",
    )
    .bind(lot)
    .execute(&pool)
    .await;
    assert!(result.is_err());
}

/// A lot's derived stock must never be negative (FR-018, issues.md 8), on every path.
///
/// The stocktake is refused in the handler, so it reads as a field error rather than a
/// database failure; the trigger underneath is what makes the rule hold for writes that never
/// reach a handler at all.
#[sqlx::test]
async fn a_stocktake_cannot_count_below_zero(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let refused = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "-1", "reason": "Vertippt" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(refused.error_fields().contains(&"new_remaining".to_owned()));

    // Nothing was written: a refused stocktake leaves the ledger as it was.
    let movements: i64 =
        sqlx::query_scalar("SELECT count(*) FROM drug_stock_movement WHERE lot_id = $1")
            .bind(lot)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(movements, 0);

    // Counting it empty is fine — zero is a stock, below zero is not.
    let emptied = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "0", "reason": "Aufgebraucht" }),
        )
        .await;
    assert_eq!(emptied.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
}

/// The same rule asked of the database, which is where it has to hold: the handler can only
/// speak for the paths that go through it.
#[sqlx::test]
async fn the_database_refuses_a_movement_that_would_go_below_zero(pool: PgPool) {
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let too_much = sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason)
         VALUES ($1, 'correction', -101, 'Direkt in die Datenbank')",
    )
    .bind(lot)
    .execute(&pool)
    .await;
    assert!(too_much.is_err(), "101 cannot come off a lot of 100");

    // Exactly the stock there is still goes through; the boundary is inclusive.
    let all_of_it = sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason)
         VALUES ($1, 'correction', -100, 'Aufgebraucht')",
    )
    .bind(lot)
    .execute(&pool)
    .await;
    assert!(
        all_of_it.is_ok(),
        "a lot may be emptied, just not overdrawn"
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
}
