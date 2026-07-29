//! Scheduled maintenance: picker usage refresh, abandoned-draft cleanup, orphaned
//! attachment sweep (T076).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use chrono::Utc;
use common::TestApp;
use k_vet_backend::domain::files::AttachmentStore;
use k_vet_backend::jobs;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{AssertSqlSafe, PgPool};

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

/// Ages a row so the cleanup considers it abandoned. The table names are test literals.
async fn age(pool: &PgPool, table: &str, id: i64, hours: i64) {
    let sql =
        format!("UPDATE {table} SET created_at = now() - ($2 || ' hours')::interval WHERE id = $1");
    sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(hours.to_string())
        .execute(pool)
        .await
        .expect("age row");
}

async fn exists(pool: &PgPool, table: &str, id: i64) -> bool {
    let sql = format!("SELECT true FROM {table} WHERE id = $1");
    sqlx::query_scalar::<_, bool>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .expect("exists")
        .is_some()
}

#[sqlx::test]
async fn the_picker_usage_table_is_rebuilt_from_the_billed_lines(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let service_id = common::seed_got_service(&pool).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;

    // The service is billed twice, the drug once.
    for _ in 0..2 {
        app.post(
            &format!("/api/treatments/{treatment_id}/items"),
            json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
        )
        .await;
    }
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({
            "kind": "drug_packaging",
            "drug_packaging_id": drug.subset_packaging_id,
            "quantity": "1"
        }),
    )
    .await;

    // A stale weight that no longer matches reality has to disappear.
    sqlx::query("INSERT INTO picker_usage (kind, item_id, uses) VALUES ('service', $1, 999)")
        .bind(service_id + 10_000)
        .execute(&pool)
        .await
        .expect("stale weight");

    let rows = jobs::refresh_picker_usage(&pool).await.expect("refresh");
    assert_eq!(rows, 2, "one row per used item");

    let service_uses: i64 =
        sqlx::query_scalar("SELECT uses FROM picker_usage WHERE kind = 'service' AND item_id = $1")
            .bind(service_id)
            .fetch_one(&pool)
            .await
            .expect("service uses");
    assert_eq!(service_uses, 2);

    let drug_uses: i64 = sqlx::query_scalar(
        "SELECT uses FROM picker_usage WHERE kind = 'drug_packaging' AND item_id = $1",
    )
    .bind(drug.subset_packaging_id)
    .fetch_one(&pool)
    .await
    .expect("drug uses");
    assert_eq!(drug_uses, 1);

    let stale: i64 = sqlx::query_scalar("SELECT count(*) FROM picker_usage WHERE uses = 999")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(stale, 0, "the rebuild replaces the whole table");
}

#[sqlx::test]
async fn untouched_drafts_are_removed_after_a_day(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // Opened by accident and left alone: no field was ever filled in.
    let abandoned = app.post("/api/customers", json!({})).await.id();
    let abandoned_patient = app.post("/api/patients", json!({})).await.id();
    let abandoned_service = app
        .post("/api/services", json!({ "type": "self_defined" }))
        .await
        .id();
    let abandoned_template = app.post("/api/treatment-templates", json!({})).await.id();
    let abandoned_drug = app.post("/api/drugs", json!({})).await.id();
    let abandoned_supplier = app.post("/api/suppliers", json!({})).await.id();
    let abandoned_appointment = app.post("/api/appointments", json!({})).await.id();

    for (table, id) in [
        ("customer", abandoned),
        ("patient", abandoned_patient),
        ("service", abandoned_service),
        ("treatment_template", abandoned_template),
        ("drug", abandoned_drug),
        ("supplier", abandoned_supplier),
        ("appointment", abandoned_appointment),
    ] {
        age(&pool, table, id, 30).await;
    }

    // Just as old, but the vet started writing — that draft is work in progress.
    let started = app.post("/api/customers", json!({})).await.id();
    app.patch(
        &format!("/api/customers/{started}"),
        json!({ "last_name": "Müller" }),
    )
    .await;
    age(&pool, "customer", started, 30).await;

    // Untouched, but not yet a day old.
    let fresh = app.post("/api/customers", json!({})).await.id();

    let removed = jobs::cleanup_abandoned_drafts(&pool)
        .await
        .expect("cleanup");
    assert_eq!(removed.total(), 7, "one per abandoned draft: {removed:?}");

    assert!(!exists(&pool, "customer", abandoned).await);
    assert!(!exists(&pool, "patient", abandoned_patient).await);
    assert!(!exists(&pool, "service", abandoned_service).await);
    assert!(!exists(&pool, "treatment_template", abandoned_template).await);
    assert!(!exists(&pool, "drug", abandoned_drug).await);
    assert!(!exists(&pool, "supplier", abandoned_supplier).await);
    assert!(!exists(&pool, "appointment", abandoned_appointment).await);

    assert!(
        exists(&pool, "customer", started).await,
        "a started draft is kept"
    );
    assert!(
        exists(&pool, "customer", fresh).await,
        "today's draft is kept"
    );
}

#[sqlx::test]
async fn a_draft_with_children_is_never_removed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;

    // An empty customer draft that already has an email address and a patient attached.
    let customer_id = app.post("/api/customers", json!({})).await.id();
    app.post(
        &format!("/api/customers/{customer_id}/emails"),
        json!({ "email": "erika@example.com", "email_type": "private" }),
    )
    .await;
    age(&pool, "customer", customer_id, 48).await;

    // An empty drug draft with a packaging under it.
    let drug_id = app.post("/api/drugs", json!({})).await.id();
    app.post(
        &format!("/api/drugs/{drug_id}/packagings"),
        json!({ "kind": "original" }),
    )
    .await;
    age(&pool, "drug", drug_id, 48).await;

    // An appointment cannot be in this test: a treatment only attaches to a *complete*
    // appointment, so a draft appointment never has children to protect.

    jobs::cleanup_abandoned_drafts(&pool)
        .await
        .expect("cleanup");

    assert!(exists(&pool, "customer", customer_id).await);
    assert!(exists(&pool, "drug", drug_id).await);
}

#[sqlx::test]
async fn orphaned_attachments_are_swept_with_their_files(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let store = AttachmentStore::new(app.attachments.path());

    // Referenced by nothing: an upload whose form was abandoned.
    let orphan = app
        .post_multipart("/api/attachments", "logo.png", "image/png", &png(), &[])
        .await;
    let orphan_id = orphan.id();
    let orphan_sha: String = sqlx::query_scalar("SELECT sha256 FROM attachment WHERE id = $1")
        .bind(orphan_id)
        .fetch_one(&pool)
        .await
        .expect("sha");
    age(&pool, "attachment", orphan_id, 24 * 40).await;

    // The practice logo is referenced, so it stays whatever its age.
    let logo = app
        .post_multipart("/api/attachments", "praxis.png", "image/png", &png2(), &[])
        .await;
    let logo_id = logo.id();
    app.patch("/api/settings", json!({ "logo_attachment_id": logo_id }))
        .await;
    age(&pool, "attachment", logo_id, 24 * 40).await;

    // A young orphan is left alone: its form may still be open in another tab.
    let young = app
        .post_multipart("/api/attachments", "neu.png", "image/png", &png3(), &[])
        .await;
    let young_id = young.id();

    let swept = jobs::sweep_orphan_attachments(&pool, &store)
        .await
        .expect("sweep");
    assert_eq!(swept, 1, "only the aged orphan");

    assert!(!exists(&pool, "attachment", orphan_id).await);
    assert!(exists(&pool, "attachment", logo_id).await);
    assert!(exists(&pool, "attachment", young_id).await);
    assert!(
        !store.content_path(&orphan_sha).exists(),
        "the file goes with the row"
    );
}

/// Three distinct PNGs: identical content would be deduplicated into one attachment.
fn png() -> Vec<u8> {
    png_with(20, 90, 70)
}
fn png2() -> Vec<u8> {
    png_with(27, 54, 93)
}
fn png3() -> Vec<u8> {
    png_with(155, 80, 35)
}

fn png_with(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbImage::from_pixel(8, 8, image::Rgb([r, g, b]));
    image::DynamicImage::ImageRgb8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encodes png");
    bytes
}
