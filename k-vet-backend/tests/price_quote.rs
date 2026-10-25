//! Preisauskunft: a list of positions priced exactly as a treatment and its invoice price them,
//! with nothing written; and the kilometres for its Wegegeld.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::{FakeGoogle, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn quote(app: &TestApp, lines: Value) -> Value {
    let response = app
        .post("/api/price-quote", json!({ "lines": lines }))
        .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    response.json()
}

/// A self-defined service at the reduced rate, so a quote has two VAT groups.
async fn seed_reduced_service(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, draft)
         VALUES ('self_defined', 'Futterberatung', 7.000, 18.40, false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn row_count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn a_quote_comes_to_what_the_invoice_would(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let reduced = seed_reduced_service(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let drug = common::seed_drug(&pool).await;
    // Stock for the dispense the treatment books; the quote itself books none.
    common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        rust_decimal::Decimal::from(100),
        None,
    )
    .await;

    let lines = json!([
        { "kind": "service", "id": got, "quantity": "1", "factor": "230" },
        { "kind": "drug_packaging", "id": drug.subset_packaging_id, "quantity": "2" },
        { "kind": "service", "id": reduced, "quantity": "1" },
        { "kind": "service", "id": wegegeld, "quantity": "1", "km": "12", "km_multiplier": "2" },
    ]);

    let counted = [
        "treatment_item",
        "appointment",
        "invoice",
        "drug_stock_movement",
    ];
    let mut before = Vec::new();
    for table in counted {
        before.push(row_count(&pool, table).await);
    }
    let priced = quote(&app, lines).await;
    for (table, count) in counted.iter().zip(before) {
        assert_eq!(
            row_count(&pool, table).await,
            count,
            "a quote writes no {table}"
        );
    }

    // The same positions on a real visit.
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    let add = |body: Value| {
        let app = &app;
        async move {
            let response = app
                .post(&format!("/api/treatments/{treatment_id}/items"), body)
                .await;
            assert_eq!(response.status, StatusCode::OK);
            response.json()
        }
    };
    let got_line = add(json!({ "kind": "service", "service_id": got, "quantity": "1" })).await;
    app.patch(
        &format!("/api/treatment-items/{}", got_line["id"]),
        json!({ "factor": "230" }),
    )
    .await;
    add(json!({
        "kind": "drug_packaging", "drug_packaging_id": drug.subset_packaging_id, "quantity": "2"
    }))
    .await;
    add(json!({ "kind": "service", "service_id": reduced, "quantity": "1" })).await;
    add(json!({
        "kind": "service", "service_id": wegegeld, "quantity": "1",
        "km": "12", "km_multiplier": "2"
    }))
    .await;

    let items = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    let items = items.as_array().unwrap();
    for line in priced["lines"].as_array().unwrap() {
        let item = items
            .iter()
            .find(|item| item["name"] == line["name"])
            .unwrap_or_else(|| panic!("no treatment line {}", line["name"]));
        for field in [
            "price_net",
            "price_gross",
            "line_net",
            "line_gross",
            "factor",
        ] {
            assert_eq!(line[field], item[field], "{} {field}", line["name"]);
        }
    }

    let treatment = app
        .get(&format!("/api/treatments/{treatment_id}"))
        .await
        .json();
    assert_eq!(priced["total_gross"], treatment["total_gross"]);
    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({ "includes_finding": true }),
        )
        .await;
    assert_eq!(invoice.status, StatusCode::OK);
    assert_eq!(priced["total_gross"], invoice.json()["total_gross"]);

    // GOT 1 at 2.3×: 23.62 → 54.33 net; subset 2 × 2.50; 18.40 at 7 %; Wegegeld 12 km × 3.50 × 2.
    assert_eq!(priced["lines"][0]["line_net"], "54.33");
    assert_eq!(priced["lines"][1]["line_net"], "5.00");
    assert_eq!(priced["lines"][3]["price_net"], "84.00");
    assert_eq!(priced["vat_groups"].as_array().unwrap().len(), 2);
    assert_eq!(priced["vat_groups"][0]["vat_percent"], "19.000");
    assert_eq!(priced["vat_groups"][1]["vat_percent"], "7.000");
    assert_eq!(priced["total_net"], "161.73");
}

#[sqlx::test]
async fn wegegeld_without_kilometres_costs_the_catalogue_price(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let house_call = common::seed_visit_service(&pool).await;

    let priced = quote(
        &app,
        json!([
            { "kind": "service", "id": wegegeld, "quantity": "1" },
            // Kilometres on a service that is not Wegegeld change nothing.
            { "kind": "service", "id": house_call, "quantity": "1", "km": "40" },
        ]),
    )
    .await;
    assert_eq!(priced["lines"][0]["price_net"], "13.00");
    assert_eq!(priced["lines"][0]["travel_expenses"], true);
    assert_eq!(priced["lines"][1]["price_net"], "34.50");
    assert_eq!(priced["lines"][1]["km"], Value::Null);

    let empty = quote(&app, json!([])).await;
    assert_eq!(empty["total_gross"], "0");
    assert_eq!(empty["vat_groups"], json!([]));
}

#[sqlx::test]
async fn a_position_that_cannot_be_priced_is_named(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let draft: i64 = sqlx::query_scalar(
        "INSERT INTO service (type, name, draft) VALUES ('self_defined', 'Entwurf', true) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let refused = |lines: Value| {
        let app = &app;
        async move {
            let response = app
                .post("/api/price-quote", json!({ "lines": lines }))
                .await;
            assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
            let errors = response.json()["errors"].clone();
            (
                errors[0]["field"].as_str().unwrap().to_owned(),
                errors[0]["message"].as_str().unwrap().to_owned(),
            )
        }
    };
    let ok = json!({ "kind": "service", "id": got, "quantity": "1" });

    assert_eq!(
        refused(json!([ok, { "kind": "service", "id": 999_999, "quantity": "1" }])).await,
        ("lines.1.id".to_owned(), "record.notFound".to_owned())
    );
    assert_eq!(
        refused(json!([{ "kind": "service", "id": draft, "quantity": "1" }])).await,
        ("lines.0.id".to_owned(), "record.incomplete".to_owned())
    );
    assert_eq!(
        refused(json!([ok, { "kind": "service", "id": got, "quantity": "0" }])).await,
        (
            "lines.1.quantity".to_owned(),
            "value.mustBePositive".to_owned()
        )
    );
    assert_eq!(
        refused(json!([{ "kind": "service", "id": got, "quantity": "1", "factor": "0" }])).await,
        (
            "lines.0.factor".to_owned(),
            "value.mustBePositive".to_owned()
        )
    );
    assert_eq!(
        refused(json!([{ "kind": "service", "id": got, "quantity": "1", "km": "-3" }])).await,
        (
            "lines.0.km".to_owned(),
            "value.mustNotBeNegative".to_owned()
        )
    );
    let many: Vec<Value> = (0..101).map(|_| ok.clone()).collect();
    assert_eq!(
        refused(json!(many)).await,
        ("lines".to_owned(), "quote.tooManyLines".to_owned())
    );
}

#[sqlx::test]
async fn a_customer_gets_the_kilometres_of_their_last_visit(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = TestApp::with_config(pool.clone(), |config| google.configure(config)).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now() - Duration::days(9)).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    let line = app
        .post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "17" }),
        )
        .await;
    assert_eq!(line.status, StatusCode::OK);

    let asked = |customer_id: i64| {
        let app = &app;
        async move {
            app.post(
                "/api/price-quote/distance",
                json!({ "customer_id": customer_id }),
            )
            .await
            .json()["km_suggestion"]
                .clone()
        }
    };
    let suggestion = asked(customer_id).await;
    assert_eq!(suggestion["source"], "previous");
    assert_eq!(suggestion["km"], "17.00");
    assert!(google.requests().is_empty());

    // Moved since: Google — and asking does not clear the mark, only a real line does.
    app.patch(
        &format!("/api/customers/{customer_id}"),
        json!({ "home_street": "Neue Straße 1" }),
    )
    .await;
    let suggestion = asked(customer_id).await;
    assert_eq!(suggestion["source"], "route");
    assert_eq!(suggestion["km"], "12");
    let outdated: bool =
        sqlx::query_scalar("SELECT travel_distance_outdated FROM customer WHERE id = $1")
            .bind(customer_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(outdated);

    let unknown = app
        .post(
            "/api/price-quote/distance",
            json!({ "customer_id": 999_999 }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn a_new_caller_s_address_is_asked_of_google(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = TestApp::with_config(pool.clone(), |config| google.configure(config)).await;
    common::seed_practice_address(&pool).await;

    let asked = app
        .post(
            "/api/price-quote/distance",
            json!({ "street": "Wegegeldweg 3", "zip": "12345", "city": "Musterstadt" }),
        )
        .await
        .json();
    assert_eq!(asked["km_suggestion"]["source"], "route");
    assert_eq!(asked["km_suggestion"]["km"], "12");
    assert_eq!(
        google.requests()[0].body["destination"]["address"],
        "Wegegeldweg 3, 12345 Musterstadt, Germany"
    );

    let partial = app
        .post(
            "/api/price-quote/distance",
            json!({ "street": "Wegegeldweg 3", "zip": "", "city": "" }),
        )
        .await
        .json();
    assert_eq!(partial["km_suggestion"]["problem"], "no_customer_address");
}

#[sqlx::test]
async fn without_a_key_only_an_earlier_visit_can_say(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;

    let by_customer = app
        .post(
            "/api/price-quote/distance",
            json!({ "customer_id": customer_id }),
        )
        .await
        .json();
    assert_eq!(by_customer["km_suggestion"], Value::Null);
    let by_address = app
        .post(
            "/api/price-quote/distance",
            json!({ "street": "Wegegeldweg 3", "zip": "12345", "city": "Musterstadt" }),
        )
        .await
        .json();
    assert_eq!(by_address["km_suggestion"], Value::Null);
}
