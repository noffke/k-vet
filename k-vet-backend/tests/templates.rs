//! Treatment templates: ordered lines, reordering, and applying them to a treatment with
//! current prices pinned in order (T059).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn ids(items: &Value) -> Vec<i64> {
    items
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["id"].as_i64())
                .collect()
        })
        .unwrap_or_default()
}

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
async fn a_template_completes_with_its_name(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post_empty("/api/treatment-templates").await;
    assert_eq!(created.json()["draft"], true);
    assert_eq!(created.json()["missing_fields"], json!(["name"]));

    let named = app
        .patch(
            &format!("/api/treatment-templates/{}", created.id()),
            json!({ "name": "Impfung Hund" }),
        )
        .await
        .json();
    assert_eq!(named["draft"], false);
    assert_eq!(named["item_count"], 0);
}

#[sqlx::test]
async fn template_lines_reference_the_catalog_without_a_price(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let service_id = common::seed_got_service(&pool).await;
    let template_id = app.post_empty("/api/treatment-templates").await.id();
    app.patch(
        &format!("/api/treatment-templates/{template_id}"),
        json!({ "name": "Impfung Hund" }),
    )
    .await;

    let with_service = app
        .post(
            &format!("/api/treatment-templates/{template_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await;
    assert_eq!(
        with_service.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&with_service.body)
    );

    let items = app
        .post(
            &format!("/api/treatment-templates/{template_id}/items"),
            json!({ "kind": "drug_packaging", "drug_packaging_id": drug.subset_packaging_id,
                    "quantity": "2" }),
        )
        .await
        .json();

    assert_eq!(items[0]["position"], 1);
    assert_eq!(items[0]["name"], "Allgemeine Untersuchung");
    assert_eq!(items[1]["position"], 2);
    assert_eq!(items[1]["quantity"], "2.00");
    assert_eq!(items[1]["unit"], "ml", "the unit comes from the packaging");
    assert!(
        items[1]["name"]
            .as_str()
            .unwrap_or_default()
            .contains("10 ml"),
        "the line says which packaging it means: {items:?}"
    );
}

#[sqlx::test]
async fn lines_can_be_reordered_and_removed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_got_service(&pool).await;
    let other_service: i64 = sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, draft)
         VALUES ('self_defined', 'Zweite Leistung', 19.000, 10.00, false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed service");
    let third_service: i64 = sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, draft)
         VALUES ('self_defined', 'Dritte Leistung', 19.000, 10.00, false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed service");

    let template_id = app.post_empty("/api/treatment-templates").await.id();
    app.patch(
        &format!("/api/treatment-templates/{template_id}"),
        json!({ "name": "Vorsorge" }),
    )
    .await;
    for service in [service_id, other_service, third_service] {
        app.post(
            &format!("/api/treatment-templates/{template_id}/items"),
            json!({ "kind": "service", "service_id": service, "quantity": "1" }),
        )
        .await;
    }
    let items = app
        .get(&format!("/api/treatment-templates/{template_id}/items"))
        .await
        .json();
    let original = ids(&items);

    let last = original[2];
    let moved = app
        .post(
            &format!("/api/template-items/{last}/move"),
            json!({ "direction": "top" }),
        )
        .await
        .json();
    assert_eq!(ids(&moved), vec![original[2], original[0], original[1]]);

    let moved = app
        .post(
            &format!("/api/template-items/{last}/move"),
            json!({ "direction": "down" }),
        )
        .await
        .json();
    assert_eq!(ids(&moved), vec![original[0], original[2], original[1]]);

    let moved = app
        .post(
            &format!("/api/template-items/{last}/move"),
            json!({ "direction": "bottom" }),
        )
        .await
        .json();
    assert_eq!(ids(&moved), vec![original[0], original[1], original[2]]);

    // Moving the first line up is a no-op, not an error.
    let unmoved = app
        .post(
            &format!("/api/template-items/{}/move", original[0]),
            json!({ "direction": "up" }),
        )
        .await;
    assert_eq!(unmoved.status, StatusCode::OK);
    assert_eq!(ids(&unmoved.json()), original);

    // Removing a line closes the gap.
    assert_eq!(
        app.delete(&format!("/api/template-items/{}", original[0]))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let remaining = app
        .get(&format!("/api/treatment-templates/{template_id}/items"))
        .await
        .json();
    assert_eq!(remaining[0]["position"], 1);
    assert_eq!(remaining[1]["position"], 2);
}

#[sqlx::test]
async fn applying_a_template_pins_current_prices_in_template_order(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, Decimal::from(100), None).await;
    let service_id = common::seed_got_service(&pool).await;

    let template_id = app.post_empty("/api/treatment-templates").await.id();
    app.patch(
        &format!("/api/treatment-templates/{template_id}"),
        json!({ "name": "Impfung Hund" }),
    )
    .await;
    app.post(
        &format!("/api/treatment-templates/{template_id}/items"),
        json!({ "kind": "drug_packaging", "drug_packaging_id": drug.subset_packaging_id,
                "quantity": "2" }),
    )
    .await;
    app.post(
        &format!("/api/treatment-templates/{template_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;

    // The price at apply time is what gets pinned.
    sqlx::query("UPDATE service SET net_price = 30.00 WHERE id = $1")
        .bind(service_id)
        .execute(&pool)
        .await
        .expect("price change");

    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    let applied = app
        .post(
            &format!("/api/treatments/{treatment_id}/apply-template"),
            json!({ "template_id": template_id }),
        )
        .await;
    assert_eq!(
        applied.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&applied.body)
    );
    let items = applied.json();

    // Template order, current prices, and the drug line dispensed FEFO.
    assert_eq!(items[0]["drug_packaging_id"], drug.subset_packaging_id);
    assert_eq!(items[0]["quantity"], "2.00");
    assert_eq!(items[0]["price_net"], "2.50");
    assert_eq!(items[0]["lots"].as_array().map(Vec::len), Some(1));
    assert_eq!(items[1]["service_id"], service_id);
    assert_eq!(
        items[1]["price_net"], "30.00",
        "the price of today, not of yesterday"
    );

    // 20 ml left the shelf.
    assert_eq!(common::lot_remaining(&pool, 1).await, Decimal::from(80));

    // Applying it again appends, it does not replace.
    let twice = app
        .post(
            &format!("/api/treatments/{treatment_id}/apply-template"),
            json!({ "template_id": template_id }),
        )
        .await
        .json();
    assert_eq!(twice.as_array().map(Vec::len), Some(4));
    assert_eq!(names(&twice).len(), 4);
}

#[sqlx::test]
async fn an_incomplete_template_cannot_be_applied(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let template_id = app.post_empty("/api/treatment-templates").await.id();
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    let rejected = app
        .post(
            &format!("/api/treatments/{treatment_id}/apply-template"),
            json!({ "template_id": template_id }),
        )
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn a_template_line_needs_a_positive_quantity(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_got_service(&pool).await;
    let template_id = app.post_empty("/api/treatment-templates").await.id();

    let rejected = app
        .post(
            &format!("/api/treatment-templates/{template_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "0" }),
        )
        .await;

    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(rejected.error_fields().contains(&"quantity".to_owned()));
}

#[sqlx::test]
async fn templates_are_archived_not_deleted(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let id = app.post_empty("/api/treatment-templates").await.id();
    app.patch(
        &format!("/api/treatment-templates/{id}"),
        json!({ "name": "Alte Gruppe" }),
    )
    .await;

    app.post_empty(&format!("/api/treatment-templates/{id}/archive"))
        .await;
    assert_eq!(
        app.get("/api/treatment-templates")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        app.get("/api/treatment-templates?archived=true")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}
