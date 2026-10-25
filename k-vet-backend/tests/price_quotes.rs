//! Preisauskünfte: positions priced exactly as a treatment and its invoice price them, kept
//! with the customer as they are put together, listed, and taken into a visit at the prices
//! quoted — with the changes since said; and the kilometres for their Wegegeld.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{Duration, Local, Utc};
use common::{FakeGoogle, TestApp};
use k_vet_backend::jobs;
use serde_json::{Value, json};
use sqlx::PgPool;

/// A quote for `customer_id` (or an open one) with `lines` saved on it.
async fn quote(app: &TestApp, customer_id: Option<i64>, lines: Value) -> Value {
    let created = app
        .post("/api/quotes", json!({ "customer_id": customer_id }))
        .await;
    assert_eq!(created.status, StatusCode::OK);
    let id = created.json()["id"].as_i64().unwrap();
    let saved = app
        .patch(&format!("/api/quotes/{id}"), json!({ "lines": lines }))
        .await;
    assert_eq!(
        saved.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    saved.json()
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

/// A visit of `customer_id` with one of their animals, `days_ago` days back.
async fn visit(pool: &PgPool, customer_id: i64) -> i64 {
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    common::seed_treatment(pool, appointment_id, patient_id).await
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
    let customer_id = common::seed_customer(&pool).await;

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
    let priced = quote(
        &app,
        Some(customer_id),
        json!([
            { "kind": "service", "id": got, "quantity": "1", "factor": "230" },
            { "kind": "drug_packaging", "id": drug.subset_packaging_id, "quantity": "2" },
            { "kind": "service", "id": reduced, "quantity": "1" },
            { "kind": "service", "id": wegegeld, "quantity": "1", "km": "12", "km_multiplier": "2" },
        ]),
    )
    .await;
    for (table, count) in counted.iter().zip(before) {
        assert_eq!(
            row_count(&pool, table).await,
            count,
            "a quote writes no {table}"
        );
    }

    // The same positions on a real visit.
    let treatment_id = visit(&pool, customer_id).await;
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
    assert_eq!(priced["total_net"], "161.73");
    assert_eq!(priced["today_total_gross"], priced["total_gross"]);
    assert_eq!(priced["prices_changed"], false);
}

#[sqlx::test]
async fn wegegeld_without_kilometres_costs_the_catalogue_price(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let house_call = common::seed_visit_service(&pool).await;

    let priced = quote(
        &app,
        None,
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

    let empty = quote(&app, None, json!([])).await;
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
    let ok = json!({ "kind": "service", "id": got, "quantity": "1" });
    let kept = quote(&app, None, json!([ok])).await;
    let id = kept["id"].as_i64().unwrap();

    let refused = |lines: Value| {
        let app = &app;
        async move {
            let response = app
                .patch(&format!("/api/quotes/{id}"), json!({ "lines": lines }))
                .await;
            assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
            let errors = response.json()["errors"].clone();
            (
                errors[0]["field"].as_str().unwrap().to_owned(),
                errors[0]["message"].as_str().unwrap().to_owned(),
            )
        }
    };

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
    // A refused save leaves the quote as it was.
    let unchanged = app.get(&format!("/api/quotes/{id}")).await.json();
    assert_eq!(unchanged["lines"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn a_quote_is_open_until_it_belongs_to_a_customer(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;

    let open = quote(
        &app,
        None,
        json!([{ "kind": "service", "id": got, "quantity": "1" }]),
    )
    .await;
    let id = open["id"].as_i64().unwrap();
    assert_eq!(open["draft"], true);
    let expected_removal = (Local::now() + Duration::days(28)).date_naive().to_string();
    assert_eq!(open["removed_on"], expected_removal.as_str());

    let named = app
        .patch(
            &format!("/api/quotes/{id}"),
            json!({
                "name": "Kastration Kater", "note": "ruft nächste Woche an",
                "address_street": "Wegegeldweg 3", "address_zip": "12345", "address_city": "Musterstadt"
            }),
        )
        .await
        .json();
    assert_eq!(named["name"], "Kastration Kater");
    assert_eq!(named["address_city"], "Musterstadt");
    assert_eq!(named["draft"], true);
    // A blank name is no name.
    let blank = app
        .patch(&format!("/api/quotes/{id}"), json!({ "note": "  " }))
        .await
        .json();
    assert_eq!(blank["note"], Value::Null);

    // A customer completes it, and their home replaces the typed address.
    let kept = app
        .patch(
            &format!("/api/quotes/{id}"),
            json!({ "customer_id": customer_id }),
        )
        .await
        .json();
    assert_eq!(kept["draft"], false);
    assert_eq!(kept["customer_name"], "Erika Mustermann");
    assert_eq!(kept["address_street"], Value::Null);
    assert_eq!(kept["removed_on"], Value::Null);
    assert_eq!(kept["lines"].as_array().unwrap().len(), 1, "the lines stay");

    let unknown = app
        .patch(
            &format!("/api/quotes/{id}"),
            json!({ "customer_id": 999_999 }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    // The database holds it too: a complete quote has a customer.
    let orphan = sqlx::query("UPDATE price_quote SET customer_id = NULL WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(orphan.is_err());

    let deleted = app.delete(&format!("/api/quotes/{id}")).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(row_count(&pool, "price_quote_line").await, 0);
    assert_eq!(
        app.get(&format!("/api/quotes/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn a_changed_catalogue_price_is_said_and_the_quote_keeps_its_own(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let kept = quote(
        &app,
        Some(customer_id),
        json!([{ "kind": "service", "id": got, "quantity": "1" }]),
    )
    .await;
    let id = kept["id"].as_i64().unwrap();

    sqlx::query("UPDATE service SET net_price = 25.00 WHERE id = $1")
        .bind(got)
        .execute(&pool)
        .await
        .unwrap();
    let later = app.get(&format!("/api/quotes/{id}")).await.json();
    let line = &later["lines"][0];
    assert_eq!(line["price_net"], "23.62", "as quoted");
    assert_eq!(line["today_price_net"], "25.00");
    assert_eq!(line["line_gross"], "28.11");
    assert_eq!(line["today_line_gross"], "29.75");
    assert_eq!(line["price_changed"], true);
    assert_eq!(later["prices_changed"], true);
    assert_eq!(later["total_gross"], "28.11");
    assert_eq!(later["today_total_gross"], "29.75");

    let listed = app
        .get(&format!("/api/customers/{customer_id}/quotes"))
        .await
        .json();
    assert_eq!(listed[0]["prices_changed"], true);
}

#[sqlx::test]
async fn a_customer_s_page_lists_their_quotes_and_the_open_ones_are_listed_apart(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let neighbour = common::seed_customer(&pool).await;
    let line = json!([{ "kind": "service", "id": got, "quantity": "1" }]);

    let first = quote(&app, Some(customer_id), line.clone()).await;
    let theirs = quote(&app, Some(neighbour), line.clone()).await;
    let second = quote(&app, Some(customer_id), line.clone()).await;
    let open = quote(&app, None, line.clone()).await;
    let newer_open = quote(&app, None, line.clone()).await;

    let listed = app
        .get(&format!("/api/customers/{customer_id}/quotes"))
        .await
        .json();
    let ids: Vec<&Value> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|row| &row["id"])
        .collect();
    assert_eq!(
        ids,
        vec![&second["id"], &first["id"]],
        "newest first, no one else's"
    );
    assert_eq!(listed[0]["positions"], json!(["Allgemeine Untersuchung"]));
    assert_eq!(listed[0]["total_gross"], "28.11");
    assert_ne!(listed[0]["id"], theirs["id"]);

    let opened = app.get("/api/quotes/open").await.json();
    let ids: Vec<&Value> = opened
        .as_array()
        .unwrap()
        .iter()
        .map(|row| &row["id"])
        .collect();
    assert_eq!(
        ids,
        vec![&newer_open["id"], &open["id"]],
        "last changed first"
    );
    assert!(opened[0]["removed_on"].is_string());

    // Given a customer, an open quote leaves the open list for theirs.
    app.patch(
        &format!("/api/quotes/{}", open["id"]),
        json!({ "customer_id": neighbour }),
    )
    .await;
    let opened = app.get("/api/quotes/open").await.json();
    assert_eq!(opened.as_array().unwrap().len(), 1);
    let theirs_now = app
        .get(&format!("/api/customers/{neighbour}/quotes"))
        .await
        .json();
    assert_eq!(theirs_now.as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn a_quote_is_taken_into_a_visit_at_the_prices_quoted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        rust_decimal::Decimal::from(100),
        None,
    )
    .await;
    let customer_id = common::seed_customer(&pool).await;
    let kept = quote(
        &app,
        Some(customer_id),
        json!([
            { "kind": "service", "id": got, "quantity": "2", "factor": "180" },
            { "kind": "drug_packaging", "id": drug.subset_packaging_id, "quantity": "1" },
            { "kind": "service", "id": wegegeld, "quantity": "1", "km": "10", "km_multiplier": "2" },
        ]),
    )
    .await;
    let quote_id = kept["id"].as_i64().unwrap();

    // Since the call: the fee went up.
    sqlx::query("UPDATE service SET net_price = 25.00 WHERE id = $1")
        .bind(got)
        .execute(&pool)
        .await
        .unwrap();

    let treatment_id = visit(&pool, customer_id).await;
    let movements = row_count(&pool, "drug_stock_movement").await;
    let applied = app
        .post(
            &format!("/api/treatments/{treatment_id}/apply-quote"),
            json!({ "quote_id": quote_id }),
        )
        .await;
    assert_eq!(
        applied.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&applied.body)
    );
    let items = applied.json();
    let by_name = |name: &str| {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"].as_str().unwrap().starts_with(name))
            .cloned()
            .unwrap_or_else(|| panic!("no line {name}"))
    };
    let got_line = by_name("Allgemeine Untersuchung");
    assert_eq!(got_line["price_net"], "23.62", "the fee as quoted");
    assert_eq!(got_line["factor"], "180.000");
    assert_eq!(got_line["quantity"], "2.00");
    let travel = by_name("Wegegeld");
    assert_eq!(travel["price_net"], "70.00");
    assert_eq!(travel["km"], "10.00");
    assert_eq!(travel["km_multiplier"], "2.000");
    assert_eq!(
        travel["patient_treatment_id"],
        Value::Null,
        "Wegegeld is the visit's"
    );
    assert!(by_name("Amoxicillin")["patient_treatment_id"].is_number());
    assert_eq!(
        row_count(&pool, "drug_stock_movement").await,
        movements + 1,
        "the drug books its dispense"
    );

    // Today's prices, when the vet chooses them.
    let other_visit = visit(&pool, customer_id).await;
    let today = app
        .post(
            &format!("/api/treatments/{other_visit}/apply-quote"),
            json!({ "quote_id": quote_id, "prices": "today" }),
        )
        .await
        .json();
    let today_got = today
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Allgemeine Untersuchung")
        .cloned()
        .unwrap();
    assert_eq!(today_got["price_net"], "25.00");
}

#[sqlx::test]
async fn a_quote_goes_only_into_its_customer_s_open_visit(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let got = common::seed_got_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let neighbour = common::seed_customer(&pool).await;
    let line = json!([{ "kind": "service", "id": got, "quantity": "1" }]);
    let kept = quote(&app, Some(customer_id), line.clone()).await;
    let open = quote(&app, None, line.clone()).await;
    let apply = |treatment_id: i64, quote_id: &Value| {
        let app = &app;
        let quote_id = quote_id.clone();
        async move {
            app.post(
                &format!("/api/treatments/{treatment_id}/apply-quote"),
                json!({ "quote_id": quote_id }),
            )
            .await
        }
    };

    let theirs = visit(&pool, neighbour).await;
    let refused = apply(theirs, &kept["id"]).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refused.json()["errors"][0]["message"],
        "quote.otherCustomer"
    );

    let own = visit(&pool, customer_id).await;
    let refused = apply(own, &open["id"]).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["errors"][0]["message"], "record.incomplete");

    // Released, the visit takes nothing more.
    assert_eq!(apply(own, &kept["id"]).await.status, StatusCode::OK);
    let invoice = app
        .post(
            &format!("/api/treatments/{own}/invoice"),
            json!({ "includes_finding": true }),
        )
        .await
        .json();
    let accepted = app
        .post(
            &format!("/api/invoices/{}/accept", invoice["id"]),
            json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(apply(own, &kept["id"]).await.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn an_open_quote_is_cleaned_up_28_days_after_its_last_change(pool: PgPool) {
    let customer_id = common::seed_customer(&pool).await;
    let insert = |customer: Option<i64>, days_ago: i64| {
        let pool = pool.clone();
        async move {
            let at = Utc::now() - Duration::days(days_ago);
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO price_quote (customer_id, draft, created_at, updated_at)
                 VALUES ($1, $2, $3, $3) RETURNING id",
            )
            .bind(customer)
            .bind(customer.is_none())
            .bind(at)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let stale = insert(None, 29).await;
    let recent = insert(None, 27).await;
    let kept = insert(Some(customer_id), 400).await;

    let removed = jobs::cleanup_abandoned_drafts(&pool).await.unwrap();
    assert_eq!(removed.quotes, 1);
    let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM price_quote ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, vec![recent, kept]);
    assert!(!left.contains(&stale));
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
