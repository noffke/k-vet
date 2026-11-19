//! A drug position is what was dispensed (migration 0039): the drug's original pack, the
//! quantity in its base unit, billed as whole packs plus the rest as a Teilmenge — requirements/
//! prices.md, `review.md` AMP-06.
//!
//! The seeded drug is a 100 ml bottle at 12,50 € net (14,88 € gross) with a 10 ml Teilmenge at
//! 2,50 € net (2,98 € gross), listed at 10,00 €.

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

/// A treatment with one animal; returns the treatment.
async fn treatment(pool: &PgPool) -> i64 {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    common::seed_treatment(pool, appointment_id, patient_id).await
}

async fn dispense(app: &TestApp, treatment_id: i64, body: Value) -> common::TestResponse {
    app.post(&format!("/api/treatments/{treatment_id}/items"), body)
        .await
}

fn refusal(response: &common::TestResponse) -> Value {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    response.json()["errors"].clone()
}

async fn set_freely_divisible(app: &TestApp, drug_id: i64) {
    let patched = app
        .patch(
            &format!("/api/drugs/{drug_id}"),
            json!({ "freely_divisible": true }),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(patched.json()["freely_divisible"], true);
}

/// 150 ml from 100 ml bottles: one bottle at the pack price, 50 ml as five Teilmengen — two
/// rows with their own prices — and 150 ml off the shelf.
#[tokio::test]
async fn a_dispensed_quantity_is_billed_as_whole_packs_and_a_teilmenge() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 2, dec("100"), None).await;

    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "150" }),
    )
    .await;
    assert_eq!(line.status, StatusCode::OK);
    let line = line.json();
    assert_eq!(line["name"], "Amoxicillin 100");
    assert_eq!(line["unit"], "ml");
    assert_eq!(line["quantity"], "150.00");
    assert_eq!(line["price_net"], "12.50", "the pinned price is one pack");
    assert_eq!(line["price_gross"], "14.88");
    assert_eq!(
        line["dispense"],
        json!({
            "rule": { "pack_size": "100.00", "step": "10.00", "freely_divisible": false,
                      "preset": "10.00" },
            "packs": { "quantity": "1", "price_gross": "14.88", "line_net": "12.50",
                       "line_gross": "14.88" },
            "remainder": { "quantity": "50.00", "price_gross": "0.298", "line_net": "12.50",
                           "line_gross": "14.88" },
        })
    );
    assert_eq!(line["line_net"], "25.00");
    assert_eq!(
        line["line_gross"], "29.76",
        "the rows, VAT rounded per row, added up"
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("50"));

    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({ "includes_finding": true }),
        )
        .await;
    assert_eq!(invoice.status, StatusCode::OK);
    assert_eq!(invoice.json()["total_gross"], "29.76");
}

/// Without "Beliebig teilbar" the rest goes in steps of the smallest Teilmenge, on adding and on
/// changing a line; with it, any amount — 2,7 ml of an injection.
#[tokio::test]
async fn the_rest_goes_in_teilmenge_steps_unless_the_drug_is_freely_divisible() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let off_step = json!([{ "field": "quantity", "message": "item.subsetStep" }]);

    for quantity in ["15", "2.7", "105"] {
        let refused = dispense(
            &app,
            treatment_id,
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                    "quantity": quantity }),
        )
        .await;
        assert_eq!(refusal(&refused), off_step, "{quantity} ml");
    }
    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "20" }),
    )
    .await
    .json();
    let changed = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "quantity": "25" }),
        )
        .await;
    assert_eq!(refusal(&changed), off_step);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("80"));

    // "Beliebig teilbar" takes the place of the Teilmenge (migration 0041).
    let deleted = app
        .delete(&format!("/api/packagings/{}", drug.subset_packaging_id))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    set_freely_divisible(&app, drug.drug_id).await;
    let injection = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "2.7" }),
    )
    .await;
    assert_eq!(injection.status, StatusCode::OK);
    let injection = injection.json();
    // The whole bottle priced as a Teilmenge: 10,00 € listed plus 100 % = 20,00 € for 100 ml,
    // 0,20 € per ml. 2,7 ml = 0,54 € net.
    assert_eq!(injection["line_net"], "0.54");
    assert_eq!(injection["dispense"]["rule"]["freely_divisible"], true);
    assert_eq!(injection["dispense"]["rule"]["step"], Value::Null);
    assert_eq!(injection["dispense"]["rule"]["preset"], "1");
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("77.3"));

    // The line entered before the flag keeps the rule it was entered by.
    let still = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "quantity": "25" }),
        )
        .await;
    assert_eq!(refusal(&still), off_step);
}

/// A drug without a Teilmenge goes in whole packs — unless it is "beliebig teilbar", when the
/// rest takes its share of the whole pack priced as a Teilmenge (§ 4 by analogy, 100 %).
#[tokio::test]
async fn a_drug_without_a_teilmenge_goes_in_whole_packs() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    sqlx::query("UPDATE drug_packaging SET archived = true WHERE id = $1")
        .bind(drug.subset_packaging_id)
        .execute(&pool)
        .await
        .expect("no usable Teilmenge");
    let lot = common::seed_lot(&pool, drug.packaging_id, 3, dec("100"), None).await;

    let refused = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "150" }),
    )
    .await;
    assert_eq!(
        refusal(&refused),
        json!([{ "field": "quantity", "message": "item.wholePacksOnly" }])
    );

    // Left out, the quantity is one bottle.
    let bottle = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id }),
    )
    .await
    .json();
    assert_eq!(bottle["quantity"], "100.00");
    assert_eq!(bottle["dispense"]["remainder"], Value::Null);
    assert_eq!(bottle["line_net"], "12.50");

    set_freely_divisible(&app, drug.drug_id).await;
    // 10,00 € listed for 100 ml, plus 100 %: 0,20 € per ml; 2,7 ml = 0,54 € net.
    let injection = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "2.7" }),
    )
    .await
    .json();
    assert_eq!(injection["line_net"], "0.54");
    assert_eq!(injection["dispense"]["remainder"]["price_gross"], "0.238");
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("197.3"));
}

/// A new line starts at one unit where one unit can be dispensed, else at the smallest Teilmenge.
#[tokio::test]
async fn a_new_line_starts_at_what_the_drug_allows() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;

    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id }),
    )
    .await
    .json();
    assert_eq!(line["quantity"], "10.00", "one 10 ml Teilmenge");

    sqlx::query(
        "INSERT INTO drug_packaging (drug_id, kind, unit, quantity, list_price_net,
                                     sales_price_net, draft)
         VALUES ($1, 'subset', 'ml', 1.00, 0.10, 0.25, false)",
    )
    .bind(drug.drug_id)
    .execute(&pool)
    .await
    .expect("a 1 ml Teilmenge");
    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id }),
    )
    .await
    .json();
    assert_eq!(
        line["quantity"], "1.00",
        "the smallest Teilmenge sets the step"
    );
    assert_eq!(line["dispense"]["rule"]["step"], "1.00");
}

/// The Teilmenge row's price can be typed per unit, gross; it is stored for the Teilmenge as
/// pinned, so the steps stay.
#[tokio::test]
async fn the_teilmenge_price_can_be_typed_per_unit() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 2, dec("100"), None).await;
    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id,
                "quantity": "150" }),
    )
    .await
    .json();

    let priced = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "subset_price_gross": "0.30" }),
        )
        .await;
    assert_eq!(priced.status, StatusCode::OK);
    let priced = priced.json();
    // 0,30 € a ml is 3,00 € for the 10 ml Teilmenge: 2,52 € net. 50 ml: 12,60 € net.
    assert_eq!(priced["dispense"]["remainder"]["price_gross"], "0.30");
    assert_eq!(priced["dispense"]["remainder"]["line_net"], "12.60");
    assert_eq!(priced["dispense"]["rule"]["step"], "10.00");
    assert_eq!(priced["price_net"], "12.50", "the pack keeps its price");
    let off_step = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "quantity": "155" }),
        )
        .await;
    assert_eq!(
        refusal(&off_step),
        json!([{ "field": "quantity", "message": "item.subsetStep" }])
    );
}

/// A line billed before migration 0039 counts packagings: it reads, prices, edits and prints as
/// it always did.
#[tokio::test]
async fn a_line_in_the_old_form_keeps_counting_packagings() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, record_id) =
        common::seed_patient_treatment(&pool, appointment_id, patient_id).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let item_id: i64 = sqlx::query_scalar(
        "INSERT INTO treatment_item
             (treatment_id, position, kind, drug_packaging_id, patient_treatment_id, per_visit,
              name, quantity, unit, price_net, vat_percent)
         VALUES ($1, 1, 'drug_packaging', $2, $3, false, 'Amoxicillin 100 · 10 ml', 3, 'ml',
                 2.50, 19)
         RETURNING id",
    )
    .bind(treatment_id)
    .bind(drug.subset_packaging_id)
    .bind(record_id)
    .fetch_one(&pool)
    .await
    .expect("a line from before 0039");

    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(items[0]["dispense"], Value::Null);
    assert_eq!(items[0]["line_net"], "7.50", "three Teilmengen at 2,50 €");

    // Two of them now: 20 ml off the shelf.
    let changed = app
        .patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "quantity": "2" }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json()["line_net"], "5.00");
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("80"));
    let typed = app
        .patch(
            &format!("/api/treatment-items/{item_id}"),
            json!({ "subset_price_gross": "0.30" }),
        )
        .await;
    assert_eq!(
        typed.status,
        StatusCode::BAD_REQUEST,
        "no Teilmenge row to price"
    );

    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({ "includes_finding": true }),
        )
        .await;
    assert_eq!(invoice.status, StatusCode::OK);
    assert_eq!(invoice.json()["total_gross"], "5.95");
}

/// Migration 0039 converts what is still open — treatment lines without a released invoice,
/// quotes, Behandlungsgruppen, Mitbringen — into base units of the original, keeping the
/// pinned prices, and leaves a released invoice's lines as they were billed.
#[sqlx::test(migrations = false)]
async fn the_migration_converts_what_is_open_and_keeps_what_was_billed(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator.run_to(38, &pool).await.expect("migrate to 0038");
    let drug = common::seed_drug(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;

    let line = |treatment_id: i64, record_id: i64, position: i32, packaging: i64, quantity: i32| {
        let pool = pool.clone();
        async move {
            let price = if packaging == drug.packaging_id {
                "12.40"
            } else {
                "2.40"
            };
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO treatment_item
                     (treatment_id, position, kind, drug_packaging_id, patient_treatment_id,
                      per_visit, name, quantity, unit, price_net, vat_percent)
                 VALUES ($1, $2, 'drug_packaging', $3, $4, false, 'Amoxicillin 100 · 10 ml',
                         $5, 'ml', $6::numeric, 19)
                 RETURNING id",
            )
            .bind(treatment_id)
            .bind(position)
            .bind(packaging)
            .bind(record_id)
            .bind(quantity)
            .bind(price)
            .fetch_one(&pool)
            .await
            .expect("a line in the old form")
        }
    };
    let open_appointment = common::seed_appointment(&pool, Utc::now()).await;
    let (open, open_record) =
        common::seed_patient_treatment(&pool, open_appointment, patient_id).await;
    let open_subset = line(open, open_record, 1, drug.subset_packaging_id, 3).await;
    let open_bottle = line(open, open_record, 2, drug.packaging_id, 1).await;
    let billed_appointment = common::seed_appointment(&pool, Utc::now()).await;
    let (billed, billed_record) =
        common::seed_patient_treatment(&pool, billed_appointment, patient_id).await;
    let billed_subset = line(billed, billed_record, 1, drug.subset_packaging_id, 2).await;
    sqlx::query(
        "INSERT INTO invoice (treatment_id, invoice_number, invoice_date, status, ts_accepted)
         VALUES ($1, '2026-0001', current_date, 'accepted', now())",
    )
    .bind(billed)
    .execute(&pool)
    .await
    .expect("a released invoice");

    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO treatment_template (name, draft) VALUES ('Impfung', false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("template");
    sqlx::query(
        "INSERT INTO treatment_template_item
             (template_id, position, kind, drug_packaging_id, quantity, unit)
         VALUES ($1, 1, 'drug_packaging', $2, 2, 'ml')",
    )
    .bind(template_id)
    .bind(drug.subset_packaging_id)
    .execute(&pool)
    .await
    .expect("template item");
    sqlx::query(
        "INSERT INTO appointment_reminder (appointment_id, drug_packaging_id, quantity, position)
         VALUES ($1, $2, 4, 1)",
    )
    .bind(open_appointment)
    .bind(drug.subset_packaging_id)
    .execute(&pool)
    .await
    .expect("reminder");
    let quote_id: i64 = sqlx::query_scalar(
        "INSERT INTO price_quote (customer_id, draft) VALUES ($1, false) RETURNING id",
    )
    .bind(customer_id)
    .fetch_one(&pool)
    .await
    .expect("quote");
    sqlx::query(
        "INSERT INTO price_quote_line
             (quote_id, position, kind, drug_packaging_id, quantity, name, price_net, vat_percent)
         VALUES ($1, 1, 'drug_packaging', $2, 1, 'Amoxicillin 100 · 10 ml', 2.40, 19)",
    )
    .bind(quote_id)
    .bind(drug.subset_packaging_id)
    .execute(&pool)
    .await
    .expect("quote line");

    migrator.run_to(39, &pool).await.expect("apply 0039");

    #[derive(sqlx::FromRow, Debug, PartialEq)]
    struct Converted {
        drug_packaging_id: i64,
        quantity: Decimal,
        name: String,
        price_net: Decimal,
        pack_size: Option<Decimal>,
        subset_size: Option<Decimal>,
        subset_price_net: Option<Decimal>,
    }
    let item = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, Converted>(
                "SELECT drug_packaging_id, quantity, name, price_net, pack_size, subset_size,
                        subset_price_net
                 FROM treatment_item WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("the line")
        }
    };
    // Three Teilmengen: 30 ml, the Teilmenge's pinned price kept, the bottle at today's price.
    assert_eq!(
        item(open_subset).await,
        Converted {
            drug_packaging_id: drug.packaging_id,
            quantity: dec("30"),
            name: "Amoxicillin 100".to_owned(),
            price_net: dec("12.50"),
            pack_size: Some(dec("100")),
            subset_size: Some(dec("10")),
            subset_price_net: Some(dec("2.40")),
        }
    );
    // A bottle: 100 ml, its pinned price kept, the Teilmenge at today's price.
    assert_eq!(
        item(open_bottle).await,
        Converted {
            drug_packaging_id: drug.packaging_id,
            quantity: dec("100"),
            name: "Amoxicillin 100".to_owned(),
            price_net: dec("12.40"),
            pack_size: Some(dec("100")),
            subset_size: Some(dec("10")),
            subset_price_net: Some(dec("2.50")),
        }
    );
    // Billed: as it was.
    assert_eq!(
        item(billed_subset).await,
        Converted {
            drug_packaging_id: drug.subset_packaging_id,
            quantity: dec("2"),
            name: "Amoxicillin 100 · 10 ml".to_owned(),
            price_net: dec("2.40"),
            pack_size: None,
            subset_size: None,
            subset_price_net: None,
        }
    );

    let template: (i64, Decimal) = sqlx::query_as(
        "SELECT drug_packaging_id, quantity FROM treatment_template_item WHERE template_id = $1",
    )
    .bind(template_id)
    .fetch_one(&pool)
    .await
    .expect("template item");
    assert_eq!(template, (drug.packaging_id, dec("20")));
    let reminder: (i64, Decimal) = sqlx::query_as(
        "SELECT drug_packaging_id, quantity FROM appointment_reminder WHERE appointment_id = $1",
    )
    .bind(open_appointment)
    .fetch_one(&pool)
    .await
    .expect("reminder");
    assert_eq!(reminder, (drug.packaging_id, dec("40")));
    let quoted: (
        i64,
        Decimal,
        Decimal,
        Option<Decimal>,
        Option<Decimal>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT drug_packaging_id, quantity, price_net, pack_size, subset_price_net, unit
             FROM price_quote_line WHERE quote_id = $1",
    )
    .bind(quote_id)
    .fetch_one(&pool)
    .await
    .expect("quote line");
    assert_eq!(
        quoted,
        (
            drug.packaging_id,
            dec("10"),
            dec("12.50"),
            Some(dec("100")),
            Some(dec("2.40")),
            Some("ml".to_owned()),
        ),
        "the quoted Teilmenge price stays"
    );
}

/// Raising a line counts what the line already holds: 10 ml raised to the whole bottle, with one
/// bottle on the shelf, goes through; one ml more is refused and leaves the line as it was.
#[tokio::test]
async fn raising_a_line_counts_what_it_already_holds() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let treatment_id = treatment(&pool).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.packaging_id }),
    )
    .await
    .json();
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("90"));

    let whole = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "quantity": "100" }),
        )
        .await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));

    let more = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "quantity": "110" }),
        )
        .await;
    assert_eq!(
        refusal(&more),
        json!([{ "field": "quantity", "message": "item.stockShort" }])
    );
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("0"));
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(
        items[0]["quantity"], "100.00",
        "the refused change is not kept"
    );
}

/// A Teilmenge goes from the drug — deleted, or archived where lines billed before migration
/// 0039 still name it, so they and their invoices keep it.
#[tokio::test]
async fn a_teilmenge_is_deleted_or_kept_for_the_lines_that_billed_it() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let packagings = |drug_id: i64| {
        let app = &app;
        async move {
            app.get(&format!("/api/drugs/{drug_id}/packagings"))
                .await
                .json()
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| row["id"].as_i64())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
    };

    // Unused: gone.
    let unused = common::seed_drug(&pool).await;
    let deleted = app
        .delete(&format!("/api/packagings/{}", unused.subset_packaging_id))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(packagings(unused.drug_id).await, vec![unused.packaging_id]);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM drug_packaging WHERE id = $1")
        .bind(unused.subset_packaging_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(left, 0);

    // Named by a line in the old form: archived, off the drug, still the line's.
    let billed = common::seed_drug(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let (treatment_id, record_id) =
        common::seed_patient_treatment(&pool, appointment_id, patient_id).await;
    sqlx::query(
        "INSERT INTO treatment_item
             (treatment_id, position, kind, drug_packaging_id, patient_treatment_id, per_visit,
              name, quantity, unit, price_net, vat_percent)
         VALUES ($1, 1, 'drug_packaging', $2, $3, false, 'Amoxicillin 100 · 10 ml', 3, 'ml',
                 2.50, 19)",
    )
    .bind(treatment_id)
    .bind(billed.subset_packaging_id)
    .bind(record_id)
    .execute(&pool)
    .await
    .expect("a line from before 0039");
    let archived = app
        .delete(&format!("/api/packagings/{}", billed.subset_packaging_id))
        .await;
    assert_eq!(archived.status, StatusCode::NO_CONTENT);
    assert_eq!(packagings(billed.drug_id).await, vec![billed.packaging_id]);
    let kept: bool = sqlx::query_scalar("SELECT archived FROM drug_packaging WHERE id = $1")
        .bind(billed.subset_packaging_id)
        .fetch_one(&pool)
        .await
        .expect("still there");
    assert!(kept);
    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(items[0]["line_net"], "7.50");
    // New lines no longer see it: whole bottles only.
    let line = dispense(
        &app,
        treatment_id,
        json!({ "kind": "drug_packaging", "drug_packaging_id": billed.packaging_id,
                "quantity": "10" }),
    )
    .await;
    assert_eq!(
        refusal(&line),
        json!([{ "field": "quantity", "message": "item.wholePacksOnly" }])
    );

    // A complete original is not deleted this way.
    let original = app
        .delete(&format!("/api/packagings/{}", billed.packaging_id))
        .await;
    assert_eq!(original.status, StatusCode::CONFLICT);
}

/// "Beliebig teilbar" and Teilmengen exclude each other: the flag waits until the Teilmengen
/// are deleted, and while it is set none can be added — in the database too (migration 0041).
#[tokio::test]
async fn beliebig_teilbar_and_teilmengen_exclude_each_other() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;

    let refused = app
        .patch(
            &format!("/api/drugs/{}", drug.drug_id),
            json!({ "freely_divisible": true }),
        )
        .await;
    assert_eq!(
        refusal(&refused),
        json!([{ "field": "freely_divisible", "message": "drug.hasSubsets" }])
    );

    app.delete(&format!("/api/packagings/{}", drug.subset_packaging_id))
        .await;
    set_freely_divisible(&app, drug.drug_id).await;

    let subset = app
        .post(
            &format!("/api/drugs/{}/packagings", drug.drug_id),
            json!({ "kind": "subset" }),
        )
        .await;
    assert_eq!(
        refusal(&subset),
        json!([{ "field": "kind", "message": "packaging.freelyDivisible" }])
    );
    let by_hand = sqlx::query(
        "INSERT INTO drug_packaging (drug_id, kind, unit, quantity, draft)
         VALUES ($1, 'subset', 'ml', 10, false)",
    )
    .bind(drug.drug_id)
    .execute(&pool)
    .await;
    assert!(by_hand.is_err(), "the database holds it too");

    // The flag taken off, a Teilmenge can be added again.
    let unset = app
        .patch(
            &format!("/api/drugs/{}", drug.drug_id),
            json!({ "freely_divisible": false }),
        )
        .await;
    assert_eq!(unset.status, StatusCode::OK);
    let subset = app
        .post(
            &format!("/api/drugs/{}/packagings", drug.drug_id),
            json!({ "kind": "subset" }),
        )
        .await;
    assert_eq!(subset.status, StatusCode::OK);
}
