//! Stock ledger: intake snapshots, derived reconciliation, append-only after invoicing,
//! corrections and batch traceability (T050).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
use crate::common;

use axum::http::StatusCode;
use chrono::Utc;
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

#[tokio::test]
async fn an_intake_snapshots_packages_times_packaging_quantity() {
    let db = common::database().await;
    let pool = db.pool();
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

/// The list price is confirmed at every Wareneingang — a changed wholesaler price is easily
/// missed. A changed one is stored with the lot, and the AMPreisV prices follow: the original's
/// sales price and the subsets', unless set by hand. A blank batch is no batch.
#[tokio::test]
async fn a_changed_list_price_comes_in_with_the_lot() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let intake = format!("/api/packagings/{}/stock-intakes", drug.packaging_id);
    let packagings = format!("/api/drugs/{}/packagings", drug.drug_id);
    let find = |all: &Value, id: i64| {
        all.as_array()
            .and_then(|list| list.iter().find(|packaging| packaging["id"] == id))
            .cloned()
            .expect("the packaging")
    };

    let lot = app
        .post(
            &intake,
            json!({ "packages_received": 1, "batch_number": "  ", "list_price_net": "12.00" }),
        )
        .await;
    assert_eq!(
        lot.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&lot.body)
    );
    assert!(lot.json()["batch_number"].is_null());

    let after = app.get(&packagings).await.json();
    let original = find(&after, drug.packaging_id);
    assert_eq!(original["list_price_net"], "12.00");
    // Priced by AMPreisV again, from the new list price.
    assert_eq!(original["sales_price_net"], original["computed_price_net"]);
    let subset = find(&after, drug.subset_packaging_id);
    assert_eq!(
        subset["list_price_net"], "1.20",
        "10 ml of the 100 ml bottle"
    );

    // A price set by hand stays; the list price still changes.
    app.patch(
        &format!("/api/packagings/{}", drug.packaging_id),
        json!({ "sales_price_net": "20.00" }),
    )
    .await;
    app.post(
        &intake,
        json!({ "packages_received": 1, "list_price_net": "13.00" }),
    )
    .await;
    let original = find(&app.get(&packagings).await.json(), drug.packaging_id);
    assert_eq!(original["list_price_net"], "13.00");
    assert_eq!(original["sales_price_net"], "20.00");

    let refused = app
        .post(
            &intake,
            json!({ "packages_received": 1, "list_price_net": "0" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        refused
            .error_fields()
            .contains(&"list_price_net".to_owned())
    );
}

/// A lot booked by mistake is taken back while untouched; once something was dispensed from it
/// or corrected, it is stock history and stays.
#[tokio::test]
async fn an_untouched_lot_can_be_taken_back() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let intake = format!("/api/packagings/{}/stock-intakes", drug.packaging_id);

    let mistaken = app
        .post(&intake, json!({ "packages_received": 3 }))
        .await
        .id();
    assert_eq!(
        app.delete(&format!("/api/lots/{mistaken}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&format!("/api/lots/{mistaken}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&format!("/api/lots/{mistaken}")).await.status,
        StatusCode::NOT_FOUND
    );

    let counted = app
        .post(&intake, json!({ "packages_received": 1 }))
        .await
        .id();
    let correction = app
        .post(
            &format!("/api/lots/{counted}/corrections"),
            json!({ "new_remaining": "90", "reason": "Bruch" }),
        )
        .await;
    assert_eq!(correction.status, StatusCode::OK);
    assert_eq!(
        app.delete(&format!("/api/lots/{counted}")).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.get(&format!("/api/lots/{counted}")).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn stock_can_only_be_booked_on_original_packagings() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn remaining_stock_reconciles_after_every_kind_of_movement() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    // Dispense 30 ml through a treatment.
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.packaging_id,
            "quantity": "30"
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

#[tokio::test]
async fn a_correction_records_the_difference_and_its_reason() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn a_correction_can_add_stock_again() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn previously_used_reasons_are_offered_alphabetically() {
    let db = common::database().await;
    let pool = db.pool();
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
    // "Ablauf" and "Verlust" are always offered, beside the ones used before.
    assert_eq!(
        reasons,
        vec!["Ablauf", "Bruch", "Verfall", "Verlust"],
        "distinct and sorted"
    );
}

/// A correction by a change rather than a count (issues.md, 2026-11-18): one broken bottle is
/// −50 ml, without counting the rest; a pack found again is +50 ml. It is booked as stated.
#[tokio::test]
async fn a_correction_can_be_a_change_instead_of_a_count() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 2, dec("100"), None).await;
    let correct = |body: Value| {
        let app = &app;
        async move {
            app.post(&format!("/api/lots/{lot}/corrections"), body)
                .await
        }
    };
    let errors = |response: common::TestResponse| response.json()["errors"].clone();

    let broken = correct(json!({ "delta": "-100", "reason": "Verlust" })).await;
    assert_eq!(broken.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("100"));
    let detail = broken.json();
    let last = detail["movements"]
        .as_array()
        .and_then(|movements| movements.last())
        .cloned()
        .expect("the change is in the history");
    assert_eq!(last["quantity"], "-100.00");
    assert_eq!(last["reason"], "Verlust");

    let found = correct(json!({ "delta": "50", "reason": "Fund" })).await;
    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("150"));

    // Nothing at all, below zero, or count and change at once are refused — on the field.
    assert_eq!(
        errors(correct(json!({ "delta": "0" })).await),
        json!([{ "field": "delta", "message": "value.mustNotBeZero" }])
    );
    assert_eq!(
        errors(correct(json!({ "delta": "-151" })).await),
        json!([{ "field": "delta", "message": "movement.wouldGoNegative" }])
    );
    assert_eq!(
        errors(correct(json!({ "delta": "-1", "new_remaining": "10" })).await),
        json!([{ "field": "delta", "message": "correction.countOrChange" }])
    );
    assert_eq!(
        errors(correct(json!({ "reason": "Inventur" })).await),
        json!([{ "field": "delta", "message": "correction.countOrChange" }])
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("150"));

    // The whole lot can go: down to exactly zero.
    let gone = correct(json!({ "delta": "-150", "reason": "Ablauf" })).await;
    assert_eq!(gone.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
}

#[tokio::test]
async fn the_lot_history_links_every_dispense_to_its_treatment_and_customer() {
    let db = common::database().await;
    let pool = db.pool();
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
            "drug_packaging_id": drug.packaging_id,
            "quantity": "10"
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
        json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
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

#[tokio::test]
async fn cancelling_an_invoice_appends_a_linked_counter_movement() {
    let db = common::database().await;
    let pool = db.pool();
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
            "drug_packaging_id": drug.packaging_id,
            "quantity": "20"
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
        json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
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

#[tokio::test]
async fn lots_are_listed_fefo_and_used_up_ones_are_hidden() {
    let db = common::database().await;
    let pool = db.pool();
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

#[tokio::test]
async fn frozen_movements_are_append_only() {
    let db = common::database().await;
    let pool = db.pool();
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
                "drug_packaging_id": drug.packaging_id,
                "quantity": "10"
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
        json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
    )
    .await;

    // Neither editing nor deleting the line may touch a frozen movement.
    assert_eq!(
        app.patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "quantity": "50" })
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

#[tokio::test]
async fn the_movement_shape_checks_hold_at_the_database_level() {
    let db = common::database().await;
    let pool = db.pool();
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
#[tokio::test]
async fn a_stocktake_cannot_count_below_zero() {
    let db = common::database().await;
    let pool = db.pool();
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
#[tokio::test]
async fn the_database_refuses_a_movement_that_would_go_below_zero() {
    let db = common::database().await;
    let pool = db.pool();
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

/// A drug the lots cannot cover is marked in the picker before it is picked, and the refusal
/// says why — on adding the line and on raising its quantity (issues.md: "a clear error").
#[tokio::test]
async fn a_dispense_beyond_the_stock_is_marked_and_refused_with_its_reason() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    let short = |items: &Value, packaging_id: i64| {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "drug_packaging" && item["id"] == packaging_id)
            .unwrap()["stock_short"]
            .clone()
    };

    let picker = app.get("/api/picker/items?q=Amoxicillin").await.json();
    assert_eq!(short(&picker, drug.packaging_id), json!(false));

    // The whole bottle goes: 100 ml of 100.
    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "100" }),
        )
        .await;
    assert_eq!(line.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));

    // The drug starts at its 10 ml Teilmenge, which the empty lot cannot give.
    let picker = app.get("/api/picker/items?q=Amoxicillin").await.json();
    assert_eq!(short(&picker, drug.packaging_id), json!(true));

    let refused = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "10" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refused.json()["errors"],
        json!([{ "field": "quantity", "message": "item.stockShort" }])
    );

    let raised = app
        .patch(
            &format!("/api/treatment-items/{}", line.json()["id"]),
            json!({ "quantity": "200" }),
        )
        .await;
    assert_eq!(raised.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        raised.json()["errors"],
        json!([{ "field": "quantity", "message": "item.stockShort" }])
    );
    // Nothing moved: the line keeps its bottle, the lot stays empty.
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
}

// ---------------------------------------------------------------------------------------------
// "Ohne Bestand": consumables billed without any stock kept (migration 0043, 2026-11-18).
// ---------------------------------------------------------------------------------------------

async fn movement_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM drug_stock_movement")
        .fetch_one(pool)
        .await
        .expect("count movements")
}

/// The seeded drug as a consumable: its Teilmenge deleted, "Ohne Bestand" and "Kein Medikament"
/// set — what migration 0045 makes of every consumable from before it.
async fn consumable(app: &TestApp, drug: &common::SeededDrug) {
    without_stock(
        app,
        drug,
        json!({ "without_stock": true, "not_a_medicine": true }),
    )
    .await;
}

/// The seeded drug with its Teilmenge deleted and `flags` set.
async fn without_stock(app: &TestApp, drug: &common::SeededDrug, flags: Value) {
    let deleted = app
        .delete(&format!("/api/packagings/{}", drug.subset_packaging_id))
        .await;
    assert!(deleted.status.is_success(), "the Teilmenge goes first");
    let marked = app
        .patch(&format!("/api/drugs/{}", drug.drug_id), flags)
        .await;
    assert_eq!(
        marked.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&marked.body)
    );
    assert_eq!(marked.json()["without_stock"], true);
}

/// A consumable goes on the invoice without any lot: picked, raised, released and cancelled,
/// and not one stock movement is written. It goes in whole packs only.
#[tokio::test]
async fn a_consumable_is_billed_without_any_stock() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    consumable(&app, &drug).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    // Offered as such, never short.
    let picker = app.get("/api/picker/items?q=Amoxicillin").await.json();
    let item = picker
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["kind"] == "drug_packaging" && item["id"] == drug.packaging_id)
        })
        .cloned()
        .expect("the consumable is offered");
    assert_eq!(item["without_stock"], true);
    assert_eq!(item["stock_short"], false);

    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "100" }),
        )
        .await;
    assert_eq!(
        line.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&line.body)
    );
    let line_id = line.id();
    let raised = app
        .patch(
            &format!("/api/treatment-items/{line_id}"),
            json!({ "quantity": "300" }),
        )
        .await;
    assert_eq!(raised.status, StatusCode::OK);
    // No Teilmenge: a rest of a pack does not go through.
    let rest = app
        .patch(
            &format!("/api/treatment-items/{line_id}"),
            json!({ "quantity": "150" }),
        )
        .await;
    assert_eq!(rest.status, StatusCode::UNPROCESSABLE_ENTITY);

    let invoice_id = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await
        .id();
    let accepted = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    let cancelled = app
        .post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
        .await;
    assert!(cancelled.status.is_success());
    assert_eq!(movement_count(&pool).await, 0, "nothing taken off a shelf");
}

/// The flag waits for what it excludes — stock on the shelf, a Teilmenge — and while it is set
/// nothing brings stock back: no Wareneingang, no correction, no Teilmenge, no "Beliebig
/// teilbar". A lot from before stays readable, and says so.
#[tokio::test]
async fn a_consumable_takes_no_stock_teilmenge_or_division() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let flag = |value: bool| json!({ "without_stock": value });
    let drug_uri = format!("/api/drugs/{}", drug.drug_id);

    let with_stock = app.patch(&drug_uri, flag(true)).await;
    assert_eq!(with_stock.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        with_stock.json()["errors"],
        json!([{ "field": "without_stock", "message": "drug.hasStock" }])
    );
    let emptied = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "0", "reason": "Inventur" }),
        )
        .await;
    assert_eq!(emptied.status, StatusCode::OK);

    let with_subset = app.patch(&drug_uri, flag(true)).await;
    assert_eq!(
        with_subset.json()["errors"],
        json!([{ "field": "without_stock", "message": "drug.withoutStockHasSubsets" }])
    );
    consumable(&app, &drug).await;

    // The lot from before is history: it says so, and takes no correction.
    let before = movement_count(&pool).await;
    let detail = app.get(&format!("/api/lots/{lot}")).await.json();
    assert_eq!(detail["drug_without_stock"], true);
    let corrected = app
        .post(
            &format!("/api/lots/{lot}/corrections"),
            json!({ "new_remaining": "5", "reason": "Fund" }),
        )
        .await;
    assert_eq!(
        corrected.json()["errors"],
        json!([{ "field": "new_remaining", "message": "lot.drugWithoutStock" }])
    );
    // Nor does the database take one written past the API.
    let written = sqlx::query(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason)
         VALUES ($1, 'correction', 5, 'roh')",
    )
    .bind(lot)
    .execute(&pool)
    .await;
    assert!(written.is_err(), "the trigger refuses it");
    assert_eq!(movement_count(&pool).await, before);

    let intake = app
        .post(
            &format!("/api/packagings/{}/stock-intakes", drug.packaging_id),
            json!({ "packages_received": 1, "batch_number": "CH-1" }),
        )
        .await;
    assert_eq!(
        intake.json()["errors"],
        json!([{ "field": "packages_received", "message": "lot.drugWithoutStock" }])
    );
    let subset = app
        .post(
            &format!("/api/drugs/{}/packagings", drug.drug_id),
            json!({ "kind": "subset" }),
        )
        .await;
    assert_eq!(
        subset.json()["errors"],
        json!([{ "field": "kind", "message": "packaging.withoutStock" }])
    );
    let divisible = app
        .patch(&drug_uri, json!({ "freely_divisible": true }))
        .await;
    assert_eq!(
        divisible.json()["errors"],
        json!([{ "field": "without_stock", "message": "drug.withoutStockNotDivisible" }])
    );

    // Turned off, it is a drug with stock again.
    assert_eq!(
        app.patch(&drug_uri, flag(false)).await.status,
        StatusCode::OK
    );
    let intake = app
        .post(
            &format!("/api/packagings/{}/stock-intakes", drug.packaging_id),
            json!({ "packages_received": 1, "batch_number": "CH-2" }),
        )
        .await;
    assert_eq!(intake.status, StatusCode::OK);
}

/// A consumable is no Arzneimittel ("Kein Medikament"): the AMPreisV does not price it. Its
/// sales price is the one the vet types, and a changed list price moves nothing; with "Kein
/// Medikament" turned off, the AMPreisV is back.
#[tokio::test]
async fn a_consumable_is_priced_by_the_vet_not_the_ampreisv() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    consumable(&app, &drug).await;
    let packaging_uri = format!("/api/packagings/{}", drug.packaging_id);
    let original = |packagings: Value| {
        packagings
            .as_array()
            .and_then(|all| all.iter().find(|row| row["kind"] == "original").cloned())
            .expect("the original")
    };
    let packagings_uri = format!("/api/drugs/{}/packagings", drug.drug_id);

    let shown = original(app.get(&packagings_uri).await.json());
    assert!(shown["computed_price_net"].is_null(), "no AMPreisV figure");

    app.patch(&packaging_uri, json!({ "list_price_net": "20.00" }))
        .await;
    let after_list = original(app.get(&packagings_uri).await.json());
    assert_eq!(
        after_list["sales_price_net"], "12.50",
        "the list price moves nothing"
    );

    app.patch(&packaging_uri, json!({ "sales_price_net": "3.00" }))
        .await;
    let typed = original(app.get(&packagings_uri).await.json());
    assert_eq!(typed["sales_price_net"], "3.00");

    // A medicine after all: the AMPreisV figure is shown again, beside the price the vet typed.
    app.patch(
        &format!("/api/drugs/{}", drug.drug_id),
        json!({ "not_a_medicine": false }),
    )
    .await;
    let drug_again = original(app.get(&packagings_uri).await.json());
    assert!(!drug_again["computed_price_net"].is_null());
    assert_eq!(drug_again["sales_price_net"], "3.00");
    assert_eq!(drug_again["price_overridden"], true);
}

/// "Ohne Bestand" alone only says that no stock is kept (migration 0045): a medicine without
/// stock is still priced by the AMPreisV.
#[tokio::test]
async fn a_medicine_without_stock_is_priced_by_the_ampreisv() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    without_stock(&app, &drug, json!({ "without_stock": true })).await;
    let packagings_uri = format!("/api/drugs/{}/packagings", drug.drug_id);
    let original = |packagings: Value| {
        packagings
            .as_array()
            .and_then(|all| all.iter().find(|row| row["kind"] == "original").cloned())
            .expect("the original")
    };

    let shown = app
        .get(&format!("/api/drugs/{}", drug.drug_id))
        .await
        .json();
    assert_eq!(shown["without_stock"], true);
    assert_eq!(shown["not_a_medicine"], false);
    // 10,00 € listed, § 3(3): 14,80 € net.
    app.patch(
        &format!("/api/packagings/{}", drug.packaging_id),
        json!({ "list_price_net": "10.00" }),
    )
    .await;
    let priced = original(app.get(&packagings_uri).await.json());
    assert_eq!(priced["computed_price_net"], "14.80");
    assert_eq!(priced["sales_price_net"], "14.80");
}

/// Mitbringen and the Packliste never call a consumable short: there is no stock to be short of.
#[tokio::test]
async fn a_consumable_to_bring_is_never_short() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    consumable(&app, &drug).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;

    let reminder = app
        .post(
            &format!("/api/appointments/{appointment_id}/reminders"),
            json!({ "drug_packaging_id": drug.packaging_id, "quantity": "200" }),
        )
        .await
        .json();
    assert_eq!(reminder["stock_short"], false);
    assert_eq!(reminder["without_stock"], true);

    let today = chrono::Local::now().date_naive();
    let packing = app
        .get(&format!("/api/packing-list?date={today}"))
        .await
        .json();
    let total = &packing["totals"][0];
    assert_eq!(total["short"], false);
    assert_eq!(total["without_stock"], true);
}

/// An invoice billed before the flag is still cancelled whole: its dispense goes back to the
/// lot it came from — the ledger staying true, not stock being kept.
#[tokio::test]
async fn an_invoice_from_before_the_flag_is_still_reversed() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id, "quantity": "100" }),
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
        json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
    )
    .await;
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
    consumable(&app, &drug).await;

    let cancelled = app
        .post_empty(&format!("/api/invoices/{invoice_id}/cancel"))
        .await;
    assert!(
        cancelled.status.is_success(),
        "{}",
        String::from_utf8_lossy(&cancelled.body)
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("100"));
}

/// Migration 0045 splits "Kein Medikament" off "Ohne Bestand": every consumable from before it
/// is "Ohne Bestand" and not a medicine, so it becomes "Kein Medikament" and keeps the price the
/// vet typed; every other drug stays a medicine.
#[sqlx::test(migrations = false)]
async fn the_migration_makes_every_consumable_no_medicine(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(44, &pool).await.expect("migrate to 0044");
    let consumable = common::seed_drug(&pool).await;
    let medicine = common::seed_drug(&pool).await;
    sqlx::query("DELETE FROM drug_packaging WHERE id = $1")
        .bind(consumable.subset_packaging_id)
        .execute(&pool)
        .await
        .expect("the Teilmenge goes first");
    sqlx::query("UPDATE drug SET without_stock = true WHERE id = $1")
        .bind(consumable.drug_id)
        .execute(&pool)
        .await
        .expect("a consumable from before 0045");

    migrator.run(&pool).await.expect("migrate to the end");

    let flags = |drug_id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (bool, bool)>(
                "SELECT without_stock, not_a_medicine FROM drug WHERE id = $1",
            )
            .bind(drug_id)
            .fetch_one(&pool)
            .await
            .expect("the drug")
        }
    };
    assert_eq!(flags(consumable.drug_id).await, (true, true));
    assert_eq!(flags(medicine.drug_id).await, (false, false));
    let price: Decimal =
        sqlx::query_scalar("SELECT sales_price_net FROM drug_packaging WHERE id = $1")
            .bind(consumable.packaging_id)
            .fetch_one(&pool)
            .await
            .expect("the pack's price");
    assert_eq!(price, Decimal::from_str_exact("12.50").expect("decimal"));
}
