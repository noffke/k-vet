//! Draft rows: the completeness invariant lives in the database (T017, research R15).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use sqlx::PgPool;

#[sqlx::test]
async fn completeness_check_rejects_completing_an_empty_row(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO customer (salutation, last_name, draft) VALUES ('frau', NULL, false)",
    )
    .execute(&pool)
    .await;

    let error = result.expect_err("a complete customer needs its mandatory fields");
    let constraint = error
        .as_database_error()
        .and_then(|error| error.constraint())
        .unwrap_or_default()
        .to_owned();
    assert_eq!(constraint, "customer_complete");
}

#[sqlx::test]
async fn draft_rows_may_be_empty(pool: PgPool) {
    let id: i64 = sqlx::query_scalar("INSERT INTO customer DEFAULT VALUES RETURNING id")
        .fetch_one(&pool)
        .await
        .expect("an empty draft is the auto-save target");

    let draft: bool = sqlx::query_scalar("SELECT draft FROM customer WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("row is readable");
    assert!(draft, "new rows start as drafts");
}

#[sqlx::test]
async fn a_complete_row_cannot_lose_a_mandatory_field(pool: PgPool) {
    let id = common::seed_customer(&pool).await;

    let result = sqlx::query("UPDATE customer SET home_zip = NULL WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;

    let error = result.expect_err("clearing a mandatory field on a complete row is rejected");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint())
            .unwrap_or_default(),
        "customer_complete"
    );
}

#[sqlx::test]
async fn patient_completeness_covers_its_own_field_set(pool: PgPool) {
    let customer_id = common::seed_customer(&pool).await;

    let result = sqlx::query(
        "INSERT INTO patient (customer_id, name, sex, draft) VALUES ($1, 'Bello', 'male', false)",
    )
    .bind(customer_id)
    .execute(&pool)
    .await;

    let error = result.expect_err("species is part of the patient completeness set");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint())
            .unwrap_or_default(),
        "patient_complete"
    );
}

#[sqlx::test]
async fn kind_bound_checks_are_draft_aware(pool: PgPool) {
    let manufacturer_id = common::seed_manufacturer(&pool).await;
    let drug_id: i64 = sqlx::query_scalar(
        "INSERT INTO drug (name, manufacturer_id, vat_percent, draft)
         VALUES ('Testarznei', $1, 19.000, false) RETURNING id",
    )
    .bind(manufacturer_id)
    .fetch_one(&pool)
    .await
    .expect("seed drug");

    // While drafting, an original packaging without a supplier is fine ...
    sqlx::query("INSERT INTO drug_packaging (drug_id, kind) VALUES ($1, 'original')")
        .bind(drug_id)
        .execute(&pool)
        .await
        .expect("drafts may be incomplete");

    // ... but completing it without a supplier is not.
    let result = sqlx::query(
        "UPDATE drug_packaging
         SET unit = 'ml', quantity = 100, list_price_net = 10, sales_price_net = 12.5,
             draft = false
         WHERE drug_id = $1",
    )
    .bind(drug_id)
    .execute(&pool)
    .await;

    let error = result.expect_err("original packagings need a supplier once complete");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint())
            .unwrap_or_default(),
        "drug_packaging_supplier_kind"
    );
}

#[sqlx::test]
async fn only_one_original_packaging_per_drug(pool: PgPool) {
    let seeded = common::seed_drug(&pool).await;

    let result = sqlx::query("INSERT INTO drug_packaging (drug_id, kind) VALUES ($1, 'original')")
        .bind(seeded.drug_id)
        .execute(&pool)
        .await;

    assert!(
        result.is_err(),
        "the partial unique index allows a single original packaging per drug"
    );
}

#[sqlx::test]
async fn stock_lots_reference_original_packagings_only(pool: PgPool) {
    let seeded = common::seed_drug(&pool).await;
    let subset_id: i64 = sqlx::query_scalar(
        "INSERT INTO drug_packaging
             (drug_id, kind, unit, quantity, list_price_net, sales_price_net, draft)
         VALUES ($1, 'subset', 'ml', 10.00, 1.00, 1.50, false) RETURNING id",
    )
    .bind(seeded.drug_id)
    .fetch_one(&pool)
    .await
    .expect("seed subset packaging");

    let result = sqlx::query(
        "INSERT INTO drug_stock_lot
             (packaging_id, packaging_kind, packages_received, initial_quantity)
         VALUES ($1, 'subset', 1, 10)",
    )
    .bind(subset_id)
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "the composite FK/CHECK pair keeps stock on originals"
    );
}
