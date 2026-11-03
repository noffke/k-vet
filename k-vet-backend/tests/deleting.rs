//! Deleting incomplete records: the one clicked into existence by mistake goes, with what hangs
//! off it; a complete one is archived instead, and one something still refers to stays.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

async fn count(pool: &PgPool, sql: &'static str, id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Every kind of record that starts as a draft can be deleted while it is one.
#[sqlx::test]
async fn an_incomplete_record_of_any_kind_is_deleted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    for collection in [
        "customers",
        "patients",
        "drugs",
        "services",
        "suppliers",
        "manufacturers",
        "treatment-templates",
        "text-blocks",
    ] {
        // A service is created with its type; everything else from nothing.
        let body = if collection == "services" {
            json!({ "type": "self_defined" })
        } else {
            json!({})
        };
        let created = app.post(&format!("/api/{collection}"), body).await;
        let id = created.id();
        assert_eq!(
            created.json()["draft"],
            true,
            "{collection} starts as a draft"
        );

        let deleted = app.delete(&format!("/api/{collection}/{id}")).await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{collection}");
        assert_eq!(
            app.get(&format!("/api/{collection}/{id}")).await.status,
            StatusCode::NOT_FOUND,
            "{collection} is gone"
        );
        assert_eq!(
            app.delete(&format!("/api/{collection}/{id}")).await.status,
            StatusCode::NOT_FOUND,
            "{collection} twice"
        );
    }
}

/// A complete record is archived, never deleted (`requirements/datamodel.md`).
#[sqlx::test]
async fn a_complete_record_is_not_deleted(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let customer_id = common::seed_customer(&pool).await;
    let manufacturer_id = common::seed_manufacturer(&pool).await;
    let drug = common::seed_drug(&pool).await;

    for uri in [
        format!("/api/customers/{customer_id}"),
        format!("/api/manufacturers/{manufacturer_id}"),
        format!("/api/drugs/{}", drug.drug_id),
    ] {
        assert_eq!(app.delete(&uri).await.status, StatusCode::CONFLICT, "{uri}");
        assert_eq!(app.get(&uri).await.status, StatusCode::OK, "{uri} stays");
    }
    assert_eq!(
        app.delete(&format!("/api/packagings/{}", drug.subset_packaging_id))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM drug_packaging WHERE id = $1",
            drug.subset_packaging_id
        )
        .await,
        1,
        "the complete packaging stays"
    );
}

/// What the draft owns goes with it: a customer's addresses, an animal's files, a template's
/// positions, a drug's packagings.
#[sqlx::test]
async fn what_belongs_to_a_draft_goes_with_it(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    let customer_id = app.post("/api/customers", json!({})).await.id();
    app.post(
        &format!("/api/customers/{customer_id}/emails"),
        json!({ "email": "irrtum@example.com", "email_type": "private" }),
    )
    .await;
    assert_eq!(
        app.delete(&format!("/api/customers/{customer_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM customer_email WHERE customer_id = $1",
            customer_id
        )
        .await,
        0
    );

    let owner = common::seed_customer(&pool).await;
    let patient_id = app
        .post("/api/patients", json!({ "customer_id": owner }))
        .await
        .id();
    sqlx::query(
        "INSERT INTO attachment (kind, patient_id, orig_name, mime_type, size_bytes, sha256)
         VALUES ('patient_file', $1, 'befund.pdf', 'application/pdf', 3, repeat('ab', 32))",
    )
    .bind(patient_id)
    .execute(&pool)
    .await
    .expect("a file on the draft animal");
    assert_eq!(
        app.delete(&format!("/api/patients/{patient_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM attachment WHERE patient_id = $1",
            patient_id
        )
        .await,
        0
    );

    let service_id = common::seed_got_service(&pool).await;
    let template_id = app.post_empty("/api/treatment-templates").await.id();
    let item = app
        .post(
            &format!("/api/treatment-templates/{template_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await;
    assert_eq!(item.status, StatusCode::OK);
    assert_eq!(
        app.delete(&format!("/api/treatment-templates/{template_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM treatment_template_item WHERE template_id = $1",
            template_id
        )
        .await,
        0
    );

    let drug_id = app.post_empty("/api/drugs").await.id();
    let packaging_id = app
        .post(
            &format!("/api/drugs/{drug_id}/packagings"),
            json!({ "kind": "original" }),
        )
        .await
        .id();
    assert_eq!(
        app.delete(&format!("/api/drugs/{drug_id}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM drug_packaging WHERE id = $1",
            packaging_id
        )
        .await,
        0
    );
}

/// A draft something still refers to stays, and the answer says it is in use — a 409, not the
/// 422 "referenced record does not exist" the foreign key would otherwise produce.
#[sqlx::test]
async fn a_draft_in_use_stays(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // A manufacturer set on a drug.
    let manufacturer_id = app.post_empty("/api/manufacturers").await.id();
    let drug_id = app.post_empty("/api/drugs").await.id();
    app.patch(
        &format!("/api/drugs/{drug_id}"),
        json!({ "manufacturer_id": manufacturer_id }),
    )
    .await;
    assert_eq!(
        app.delete(&format!("/api/manufacturers/{manufacturer_id}"))
            .await
            .status,
        StatusCode::CONFLICT
    );

    // A customer with an animal.
    let customer_id = app.post("/api/customers", json!({})).await.id();
    app.post("/api/patients", json!({ "customer_id": customer_id }))
        .await;
    assert_eq!(
        app.delete(&format!("/api/customers/{customer_id}"))
            .await
            .status,
        StatusCode::CONFLICT
    );

    // A packaging with stock: neither it nor its drug goes.
    let drug = common::seed_drug(&pool).await;
    let draft_drug_id = app.post_empty("/api/drugs").await.id();
    let packaging_id = app
        .post(
            &format!("/api/drugs/{draft_drug_id}/packagings"),
            json!({ "kind": "original" }),
        )
        .await
        .id();
    common::seed_lot(
        &pool,
        packaging_id,
        1,
        rust_decimal::Decimal::from(100),
        None,
    )
    .await;
    for uri in [
        format!("/api/packagings/{packaging_id}"),
        format!("/api/drugs/{draft_drug_id}"),
    ] {
        assert_eq!(app.delete(&uri).await.status, StatusCode::CONFLICT, "{uri}");
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM drug_packaging WHERE drug_id = $1",
            draft_drug_id
        )
        .await,
        1,
        "the refused drug keeps its packaging"
    );

    // A draft packaging nobody uses goes on its own, and its drug stays.
    let subset_id = app
        .post(
            &format!("/api/drugs/{}/packagings", drug.drug_id),
            json!({ "kind": "subset" }),
        )
        .await
        .id();
    assert_eq!(
        app.delete(&format!("/api/packagings/{subset_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&format!("/api/drugs/{}", drug.drug_id))
            .await
            .status,
        StatusCode::OK
    );
}
