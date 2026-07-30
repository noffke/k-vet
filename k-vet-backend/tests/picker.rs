//! The unified picker: ranking, exclusions and the response-time budget (T033, SC-003).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use std::time::Instant;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;
use sqlx::PgPool;

fn kinds(items: &Value) -> Vec<String> {
    items
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["kind"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Entry names; the display label is composed by the UI with locale formatting.
fn names(items: &Value) -> Vec<String> {
    items
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[sqlx::test]
async fn returns_drugs_and_services_as_a_tagged_union(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(
        &pool,
        drug.packaging_id,
        1,
        rust_decimal::Decimal::from(100),
        None,
    )
    .await;
    common::seed_got_service(&pool).await;

    let response = app.get("/api/picker/items?q=Amoxicillin").await;
    assert_eq!(response.status, StatusCode::OK);
    let drug_items = response.json();
    let services = app
        .get("/api/picker/items?q=Allgemeine%20Untersuchung")
        .await
        .json();
    let items = serde_json::Value::Array(
        drug_items
            .as_array()
            .into_iter()
            .chain(services.as_array())
            .flatten()
            .cloned()
            .collect(),
    );

    let kinds = kinds(&items);
    assert!(kinds.contains(&"drug_packaging".to_owned()), "{items:?}");
    assert!(kinds.contains(&"service".to_owned()), "{items:?}");

    let drug_item = items
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["kind"] == "drug_packaging" && item["id"] == drug.packaging_id)
        })
        .expect("the original packaging is pickable");
    assert_eq!(drug_item["name"], "Amoxicillin 100");
    assert_eq!(drug_item["quantity"], "100.00");
    assert_eq!(drug_item["unit"], "ml");
    assert_eq!(
        drug_item["in_stock"], "100.00",
        "a shortfall is visible before picking"
    );
    assert_eq!(drug_item["price_net"], "12.50");

    let subset_item = items
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item["kind"] == "drug_packaging" && item["id"] == drug.subset_packaging_id
            })
        })
        .expect("the subset packaging is pickable too");
    assert_eq!(subset_item["quantity"], "10.00");
    assert_eq!(subset_item["price_net"], "2.50");
    assert_eq!(
        subset_item["in_stock"], "100.00",
        "stock is reported per drug, in base units"
    );

    let service_item = items
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["kind"] == "service" && item["name"] == "Allgemeine Untersuchung")
        })
        .expect("the seeded GOT service is pickable");
    assert_eq!(service_item["got_number"], "1");
}

#[sqlx::test]
async fn searches_by_name_and_got_number(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    common::seed_drug(&pool).await;
    common::seed_got_service(&pool).await;

    // The drug has an original and a subset packaging; both are pickable.
    let by_name = app.get("/api/picker/items?q=Amoxi").await.json();
    assert_eq!(kinds(&by_name), vec!["drug_packaging", "drug_packaging"]);

    let by_typo = app.get("/api/picker/items?q=Amoxicilin").await.json();
    assert_eq!(
        kinds(&by_typo),
        vec!["drug_packaging", "drug_packaging"],
        "trigram search forgives typos"
    );

    let by_got = app.get("/api/picker/items?q=1").await.json();
    assert!(
        kinds(&by_got).contains(&"service".to_owned()),
        "GOT numbers are searchable"
    );

    let nothing = app.get("/api/picker/items?q=Nichtvorhanden").await.json();
    assert!(nothing.as_array().map(Vec::is_empty).unwrap_or(false));
}

#[sqlx::test]
async fn hidden_archived_and_draft_entries_are_excluded(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_got_service(&pool).await;

    sqlx::query(
        "INSERT INTO service
             (type, name, got_number, factor, vat_percent, net_price, hidden, draft)
         VALUES ('got', 'Chirurgische Leistung', '99', 100.000, 19.000, 100.00, true, false)",
    )
    .execute(&pool)
    .await
    .expect("hidden service");
    sqlx::query(
        "INSERT INTO service (type, name, vat_percent, net_price, archived, draft)
                 VALUES ('self_defined', 'Alte Leistung', 19.000, 10.00, true, false)",
    )
    .execute(&pool)
    .await
    .expect("archived service");
    sqlx::query(
        "INSERT INTO service (type, name, draft) VALUES ('self_defined', 'Halbfertig', true)",
    )
    .execute(&pool)
    .await
    .expect("draft service");
    sqlx::query("UPDATE drug_packaging SET archived = true WHERE id = $1")
        .bind(drug.subset_packaging_id)
        .execute(&pool)
        .await
        .expect("archive packaging");

    let items = app.get("/api/picker/items?q=Amoxicillin").await.json();
    let mut labels = names(&items);
    labels.extend(names(&app.get("/api/picker/items?q=Leistung").await.json()));
    labels.extend(names(
        &app.get("/api/picker/items?q=Allgemeine%20Untersuchung")
            .await
            .json(),
    ));

    assert!(
        labels
            .iter()
            .any(|label| label.contains("Allgemeine Untersuchung"))
    );
    assert!(
        !labels.iter().any(|label| label.contains("Chirurgische")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("Alte Leistung")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("Halbfertig")),
        "{labels:?}"
    );
    assert_eq!(
        labels
            .iter()
            .filter(|label| label.contains("Amoxicillin"))
            .count(),
        1,
        "the archived subset packaging is gone: {labels:?}"
    );
}

#[sqlx::test]
async fn frequently_used_entries_rank_first(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let rare: i64 = sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, draft)
         VALUES ('self_defined', 'Zzz-Sonderleistung selten', 19.000, 10.00, false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed service");
    let common_service: i64 = sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, draft)
         VALUES ('self_defined', 'Zzz-Sonderleistung häufig', 19.000, 10.00, false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed service");

    // The nightly job fills picker_usage; the ranking reads it.
    sqlx::query("INSERT INTO picker_usage (kind, item_id, uses) VALUES ('service', $1, 42)")
        .bind(common_service)
        .execute(&pool)
        .await
        .expect("usage");
    sqlx::query("INSERT INTO picker_usage (kind, item_id, uses) VALUES ('service', $1, 1)")
        .bind(rare)
        .execute(&pool)
        .await
        .expect("usage");

    let items = app
        .get("/api/picker/items?q=Zzz-Sonderleistung")
        .await
        .json();

    assert_eq!(
        names(&items),
        vec!["Zzz-Sonderleistung häufig", "Zzz-Sonderleistung selten"],
        "the practice's most-used entries come first (SC-003)"
    );
}

#[sqlx::test]
async fn ranked_results_arrive_well_within_a_second(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // A full catalog: the GOT schedule is ~1000 positions, plus the practice's drugs.
    sqlx::query(
        "INSERT INTO service (type, name, got_number, factor, vat_percent, net_price, draft)
         SELECT 'got', 'GOT Position ' || i, i::text, 100.000, 19.000, 20.00 + i, false
         FROM generate_series(1, 1200) AS i",
    )
    .execute(&pool)
    .await
    .expect("seed GOT catalog");
    let manufacturer_id = common::seed_manufacturer(&pool).await;
    sqlx::query(
        "INSERT INTO drug (name, manufacturer_id, vat_percent, draft)
         SELECT 'Medikament ' || i, $1, 19.000, false FROM generate_series(1, 300) AS i",
    )
    .bind(manufacturer_id)
    .execute(&pool)
    .await
    .expect("seed drugs");
    sqlx::query(
        "INSERT INTO drug_packaging
             (drug_id, kind, unit, quantity, list_price_net, sales_price_net, supplier_id, draft)
         SELECT id, 'original', 'Stück', 1, 5.00, 7.50, $1, false FROM drug",
    )
    .bind(common::seed_supplier(&pool).await)
    .execute(&pool)
    .await
    .expect("seed packagings");

    let started = Instant::now();
    let response = app.get("/api/picker/items?q=Position").await;
    let elapsed = started.elapsed();

    assert_eq!(response.status, StatusCode::OK);
    assert!(
        !kinds(&response.json()).is_empty(),
        "the search found matches"
    );
    assert!(
        elapsed.as_millis() < 1_000,
        "ranked results must arrive within a second (SC-003), took {elapsed:?}"
    );
}

#[sqlx::test]
async fn without_a_search_term_only_used_entries_are_offered(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    common::seed_drug(&pool).await;
    let service_id = common::seed_got_service(&pool).await;

    // A fresh practice has no history, so the empty picker is empty rather than an
    // alphabetical slice of the imported GOT catalog.
    let empty = app.get("/api/picker/items").await.json();
    assert!(
        empty.as_array().map(Vec::is_empty).unwrap_or(false),
        "expected no suggestions before anything was used: {empty:?}"
    );

    sqlx::query("INSERT INTO picker_usage (kind, item_id, uses) VALUES ('service', $1, 7)")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("usage");

    let used = app.get("/api/picker/items").await.json();
    assert_eq!(names(&used), vec!["Allgemeine Untersuchung"]);
}
