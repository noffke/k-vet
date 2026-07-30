//! Suppliers, manufacturers, drugs and the AMPreisV price computation (T052, T053).

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

/// A complete supplier and manufacturer, created through the API.
async fn address_book(app: &TestApp) -> (i64, i64) {
    let supplier = app.post_empty("/api/suppliers").await.id();
    app.patch(
        &format!("/api/suppliers/{supplier}"),
        json!({ "name": "Großhandel GmbH", "addr_street": "Lagerweg 2", "addr_zip": "10115",
                "addr_city": "Berlin" }),
    )
    .await;
    let manufacturer = app.post_empty("/api/manufacturers").await.id();
    app.patch(
        &format!("/api/manufacturers/{manufacturer}"),
        json!({ "name": "Pharma AG" }),
    )
    .await;
    (supplier, manufacturer)
}

/// A complete drug with an original packaging (100 ml at 10.00 EUR net, 19 % VAT).
async fn drug_with_original(app: &TestApp, supplier: i64, manufacturer: i64) -> (i64, i64) {
    let drug_id = app.post_empty("/api/drugs").await.id();
    app.patch(
        &format!("/api/drugs/{drug_id}"),
        json!({ "name": "Amoxicillin 100", "manufacturer_id": manufacturer, "vat_percent": "19.000" }),
    )
    .await;
    let packaging_id = app
        .post(
            &format!("/api/drugs/{drug_id}/packagings"),
            json!({ "kind": "original" }),
        )
        .await
        .id();
    app.patch(
        &format!("/api/packagings/{packaging_id}"),
        json!({ "unit": "ml", "quantity": "100", "list_price_net": "10.00",
                "supplier_id": supplier }),
    )
    .await;
    (drug_id, packaging_id)
}

fn packaging(packagings: &Value, id: i64) -> &Value {
    packagings
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == id))
        .expect("the packaging is listed")
}

#[sqlx::test]
async fn address_book_entries_complete_with_their_name(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let created = app.post_empty("/api/suppliers").await;
    assert_eq!(created.json()["draft"], true);
    assert_eq!(created.json()["missing_fields"], json!(["name"]));

    let named = app
        .patch(
            &format!("/api/suppliers/{}", created.id()),
            json!({ "name": "Großhandel GmbH" }),
        )
        .await
        .json();
    assert_eq!(named["draft"], false);
    assert_eq!(named["has_address"], false, "the address stays optional");

    let addressed = app
        .patch(
            &format!("/api/suppliers/{}", created.id()),
            json!({ "addr_street": "Lagerweg 2", "addr_zip": "10115", "addr_city": "Berlin" }),
        )
        .await
        .json();
    assert_eq!(addressed["has_address"], true);
}

#[sqlx::test]
async fn manufacturers_behave_like_suppliers_and_archive(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let id = app.post_empty("/api/manufacturers").await.id();
    app.patch(
        &format!("/api/manufacturers/{id}"),
        json!({ "name": "Pharma AG" }),
    )
    .await;

    app.post_empty(&format!("/api/manufacturers/{id}/archive"))
        .await;
    assert_eq!(
        app.get("/api/manufacturers")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(0),
        "archived entries are hidden by default"
    );
    app.post_empty(&format!("/api/manufacturers/{id}/unarchive"))
        .await;
    assert_eq!(
        app.get("/api/manufacturers")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[sqlx::test]
async fn a_drug_completes_with_name_manufacturer_and_vat(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (_, manufacturer) = address_book(&app).await;

    let created = app.post_empty("/api/drugs").await;
    assert_eq!(
        created.json()["missing_fields"],
        json!(["name", "manufacturer_id", "vat_percent"])
    );

    let complete = app
        .patch(
            &format!("/api/drugs/{}", created.id()),
            json!({ "name": "Amoxicillin 100", "manufacturer_id": manufacturer,
                    "vat_percent": "19.000", "narcotic": true, "refrigerate": true }),
        )
        .await
        .json();

    assert_eq!(complete["draft"], false);
    assert_eq!(complete["manufacturer_name"], "Pharma AG");
    assert_eq!(
        complete["narcotic"], true,
        "the flags are informational but stored"
    );
    assert_eq!(complete["refrigerate"], true);
    assert_eq!(complete["in_stock"], "0");
}

#[sqlx::test]
async fn the_sales_price_of_an_original_packaging_is_computed_per_ampreisv(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, packaging_id) = drug_with_original(&app, supplier, manufacturer).await;

    let packagings = app
        .get(&format!("/api/drugs/{drug_id}/packagings"))
        .await
        .json();
    let original = packaging(&packagings, packaging_id);

    // § 3(3) band 8.68–12.14 → 48 %: 10.00 + 4.80 = 14.80 net, shown as 17.61 gross at 19 %.
    assert_eq!(original["sales_price_net"], "14.80");
    assert_eq!(original["sales_price_gross"], "17.61");
    assert_eq!(original["computed_price_net"], "14.80");
    assert_eq!(original["price_overridden"], false);
    assert_eq!(
        original["draft"], false,
        "computing the price completes the packaging"
    );
    assert_eq!(original["supplier_name"], "Großhandel GmbH");
}

#[sqlx::test]
async fn a_subset_derives_its_list_price_and_carries_the_teilmengenzuschlag(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, _) = drug_with_original(&app, supplier, manufacturer).await;

    let subset_id = app
        .post(
            &format!("/api/drugs/{drug_id}/packagings"),
            json!({ "kind": "subset" }),
        )
        .await
        .id();
    let created = app
        .patch(
            &format!("/api/packagings/{subset_id}"),
            json!({ "unit": "ml", "quantity": "10" }),
        )
        .await;
    assert_eq!(
        created.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&created.body)
    );

    let packagings = app
        .get(&format!("/api/drugs/{drug_id}/packagings"))
        .await
        .json();
    let subset = packaging(&packagings, subset_id);

    // § 4: pro-rata basis 1.00 → +100 % → 2.00 net, shown as 2.38 gross.
    assert_eq!(
        subset["list_price_net"], "1.00",
        "derived pro rata, not typed"
    );
    assert_eq!(subset["sales_price_net"], "2.00");
    assert_eq!(subset["sales_price_gross"], "2.38");
    assert_eq!(subset["draft"], false, "a subset needs no supplier");
    assert!(subset["supplier_id"].is_null());
}

#[sqlx::test]
async fn changing_the_purchase_price_or_vat_moves_every_computed_price(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, packaging_id) = drug_with_original(&app, supplier, manufacturer).await;
    let subset_id = app
        .post(
            &format!("/api/drugs/{drug_id}/packagings"),
            json!({ "kind": "subset" }),
        )
        .await
        .id();
    app.patch(
        &format!("/api/packagings/{subset_id}"),
        json!({ "unit": "ml", "quantity": "10" }),
    )
    .await;

    // A new purchase price: 40.00 → § 3(3) 30 % → 52.00 net → 61.88 gross,
    // and the 10 ml subset follows to basis 4.00 → 8.00 net → 9.52 gross.
    app.patch(
        &format!("/api/packagings/{packaging_id}"),
        json!({ "list_price_net": "40.00" }),
    )
    .await;
    let packagings = app
        .get(&format!("/api/drugs/{drug_id}/packagings"))
        .await
        .json();
    // § 3(3) band 35.95–543.91 → 30 %: 40.00 + 12.00 = 52.00 net, 61.88 gross.
    assert_eq!(
        packaging(&packagings, packaging_id)["sales_price_net"],
        "52.00"
    );
    assert_eq!(
        packaging(&packagings, packaging_id)["sales_price_gross"],
        "61.88"
    );
    assert_eq!(packaging(&packagings, subset_id)["list_price_net"], "4.00");
    // § 4: basis 4.00 → +100 % → 8.00 net, 9.52 gross.
    assert_eq!(packaging(&packagings, subset_id)["sales_price_net"], "8.00");
    assert_eq!(
        packaging(&packagings, subset_id)["sales_price_gross"],
        "9.52"
    );

    // A VAT change moves what the customer pays, not what the practice earns. This is the
    // concrete pay-off of storing net: while prices were stored gross, dropping 19 % to 7 %
    // silently cut the margin, because the stored figure had the old rate baked into it.
    app.patch(
        &format!("/api/drugs/{drug_id}"),
        json!({ "vat_percent": "7.000" }),
    )
    .await;
    let packagings = app
        .get(&format!("/api/drugs/{drug_id}/packagings"))
        .await
        .json();
    assert_eq!(
        packaging(&packagings, packaging_id)["sales_price_net"],
        "52.00",
        "the AMPreisV net price is unaffected by the tax rate"
    );
    assert_eq!(
        packaging(&packagings, packaging_id)["sales_price_gross"],
        "55.64"
    );
    assert_eq!(packaging(&packagings, subset_id)["sales_price_net"], "8.00");
    assert_eq!(
        packaging(&packagings, subset_id)["sales_price_gross"],
        "8.56"
    );
}

#[sqlx::test]
async fn a_manual_price_wins_until_it_is_cleared(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, packaging_id) = drug_with_original(&app, supplier, manufacturer).await;

    let overridden = app
        .patch(
            &format!("/api/packagings/{packaging_id}"),
            json!({ "sales_price_net": "19.99" }),
        )
        .await
        .json();
    assert_eq!(overridden["sales_price_net"], "19.99");
    assert_eq!(overridden["price_overridden"], true);
    assert_eq!(
        overridden["computed_price_net"], "14.80",
        "the computed price stays visible next to the manual one"
    );

    // A later purchase-price change must not silently overwrite the vet's price.
    app.patch(
        &format!("/api/packagings/{packaging_id}"),
        json!({ "list_price_net": "40.00" }),
    )
    .await;
    let packagings = app
        .get(&format!("/api/drugs/{drug_id}/packagings"))
        .await
        .json();
    assert_eq!(
        packaging(&packagings, packaging_id)["sales_price_net"],
        "19.99"
    );
    assert_eq!(
        packaging(&packagings, packaging_id)["computed_price_net"],
        "52.00",
        "AMPreisV keeps computing alongside, so the vet can compare"
    );

    // Clearing the override hands the price back to AMPreisV.
    let restored = app
        .patch(
            &format!("/api/packagings/{packaging_id}"),
            json!({ "sales_price_net": null }),
        )
        .await
        .json();
    assert_eq!(restored["price_overridden"], false);
    assert_eq!(restored["sales_price_net"], "52.00");
    assert_eq!(restored["sales_price_gross"], "61.88");
}

#[sqlx::test]
async fn a_drug_has_at_most_one_original_packaging(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, _) = drug_with_original(&app, supplier, manufacturer).await;

    let second = app
        .post(
            &format!("/api/drugs/{drug_id}/packagings"),
            json!({ "kind": "original" }),
        )
        .await;

    assert!(
        second.status == StatusCode::CONFLICT || second.status == StatusCode::UNPROCESSABLE_ENTITY,
        "the partial unique index rejects a second original, got {}",
        second.status
    );
}

#[sqlx::test]
async fn the_price_preview_answers_before_anything_is_saved(pool: PgPool) {
    let app = TestApp::new(pool).await;

    let original: Value = app
        .post(
            "/api/pricing/preview",
            json!({ "kind": "original", "list_price_net": "10.00", "vat_percent": "19.000" }),
        )
        .await
        .json();
    assert_eq!(original["basis_net"], "10.00");
    assert_eq!(original["surcharge"], "4.80");
    assert_eq!(original["gross"], "17.61");
    assert!(
        original["rule"]
            .as_str()
            .unwrap_or_default()
            .contains("§ 3"),
        "the preview names the rule it applied: {original:?}"
    );

    let subset: Value = app
        .post(
            "/api/pricing/preview",
            json!({ "kind": "subset", "list_price_net": "10.00", "vat_percent": "19.000",
                    "original_quantity": "100", "subset_quantity": "10" }),
        )
        .await
        .json();
    assert_eq!(subset["basis_net"], "1.00");
    assert_eq!(subset["gross"], "2.38");
    assert!(subset["rule"].as_str().unwrap_or_default().contains("§ 4"));
}

#[sqlx::test]
async fn drugs_report_their_derived_stock(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let seeded = common::seed_drug(&pool).await;
    app.post(
        &format!("/api/packagings/{}/stock-intakes", seeded.packaging_id),
        json!({ "packages_received": 2 }),
    )
    .await;

    let drug = app
        .get(&format!("/api/drugs/{}", seeded.drug_id))
        .await
        .json();
    assert_eq!(
        drug["in_stock"], "200.00",
        "two 100 ml packages on the shelf"
    );
}

#[sqlx::test]
async fn drugs_can_be_searched_by_name_manufacturer_and_approval_number(pool: PgPool) {
    let app = TestApp::new(pool).await;
    let (supplier, manufacturer) = address_book(&app).await;
    let (drug_id, _) = drug_with_original(&app, supplier, manufacturer).await;
    app.patch(
        &format!("/api/drugs/{drug_id}"),
        json!({ "approval_number": "12345.00.00" }),
    )
    .await;

    for term in ["Amoxi", "Pharma", "12345"] {
        let found = app.get(&format!("/api/drugs?q={term}")).await.json();
        assert_eq!(
            found.as_array().map(Vec::len),
            Some(1),
            "searching for `{term}` finds the drug"
        );
    }
    assert_eq!(
        app.get("/api/drugs?q=Nichtvorhanden")
            .await
            .json()
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}
