//! The codes on a drug's packs (migration 0038) and reading a scan: whatever code of a pack is
//! scanned — EAN, PZN barcode, either Data Matrix — finds the drug, and a Data Matrix brings its
//! batch and expiry along without them ever being stored on the drug.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{Value, json};

const EAN: &str = "4006381333931";
/// The NTIN of PZN 12345678, as the securPharm coding rules give it.
const NTIN: &str = "04150123456782";

async fn scan(app: &TestApp, raw: &str) -> Value {
    let response = app.post("/api/scan", json!({ "raw": raw })).await;
    assert_eq!(response.status, StatusCode::OK);
    response.json()
}

#[tokio::test]
async fn a_code_is_added_shown_and_removed() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let uri = format!("/api/drugs/{}/codes", drug.drug_id);

    let added = app.post(&uri, json!({ "code": EAN })).await;
    assert_eq!(added.status, StatusCode::OK);
    let codes = added.json()["codes"].clone();
    assert_eq!(codes[0]["gtin"], format!("0{EAN}"));
    assert_eq!(codes[0]["display"], EAN);

    // A PZN, as typed or from its Code 39, is kept as its NTIN and shown as a PZN.
    let with_pzn = app.post(&uri, json!({ "code": "-12345678" })).await.json();
    assert_eq!(with_pzn["codes"][1]["gtin"], NTIN);
    assert_eq!(with_pzn["codes"][1]["display"], "PZN 12345678");

    // Scanned twice: still one.
    let again = app.post(&uri, json!({ "code": EAN })).await.json();
    assert_eq!(again["codes"].as_array().map(Vec::len), Some(2));

    // Typed into the Apotheke's search, a code finds its drug.
    let found = app.get(&format!("/api/drugs?q={EAN}")).await.json();
    assert_eq!(found[0]["id"], drug.drug_id);

    let code_id = again["codes"][0]["id"].as_i64().expect("an id");
    assert_eq!(
        app.delete(&format!("/api/drug-codes/{code_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&format!("/api/drug-codes/{code_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(scan(&app, EAN).await["drug"].is_null());
}

#[tokio::test]
async fn a_code_names_one_drug_and_must_be_one() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let first = common::seed_drug(&pool).await;
    let second = common::seed_drug(&pool).await;
    app.post(
        &format!("/api/drugs/{}/codes", first.drug_id),
        json!({ "code": EAN }),
    )
    .await;

    let taken = app
        .post(
            &format!("/api/drugs/{}/codes", second.drug_id),
            json!({ "code": EAN }),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CONFLICT);

    for junk in ["4006381333932", "Metacam", ""] {
        let invalid = app
            .post(
                &format!("/api/drugs/{}/codes", second.drug_id),
                json!({ "code": junk }),
            )
            .await;
        assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY, "{junk}");
        assert!(invalid.error_fields().contains(&"code".to_owned()));
    }
    assert_eq!(
        app.post("/api/drugs/999999/codes", json!({ "code": NTIN }))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

/// A Data Matrix added as a code keeps only the product: a pack of the next delivery, with
/// another batch and expiry, finds the same drug — and brings its own batch and expiry.
#[tokio::test]
async fn a_data_matrix_names_the_drug_and_brings_the_lot() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    // GS1: AI 01 the NTIN, 17 the expiry, 10 the batch (ended by GS), 21 the serial.
    let first_pack = "01041501234567821729033110CH-A1\u{1d}21SERIAL1";
    let added = app
        .post(
            &format!("/api/drugs/{}/codes", drug.drug_id),
            json!({ "code": first_pack }),
        )
        .await
        .json();
    assert_eq!(added["codes"][0]["gtin"], NTIN);
    assert_eq!(added["codes"].as_array().map(Vec::len), Some(1));

    // The next delivery, as an IFA Data Matrix this time: the PPN carries the same PZN.
    let next_pack =
        "[)>\u{1e}06\u{1d}9N111234567842\u{1d}1TCH-B2\u{1d}D300600\u{1d}SXYZ\u{1e}\u{4}";
    let read = scan(&app, next_pack).await;
    assert_eq!(read["gtin"], NTIN);
    assert_eq!(read["display"], "PZN 12345678");
    assert_eq!(read["batch"], "CH-B2");
    assert_eq!(read["expiry"], "2030-06-30");
    assert_eq!(read["drug"]["id"], drug.drug_id);
    assert_eq!(read["original"]["id"], drug.packaging_id);
    assert_eq!(read["original"]["list_price_net"], "10.00");

    // Its PZN barcode alone finds it too.
    assert_eq!(scan(&app, "-12345678").await["drug"]["id"], drug.drug_id);
}

#[tokio::test]
async fn a_scan_of_nothing_known_says_so() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let unknown = scan(&app, EAN).await;
    assert_eq!(unknown["gtin"], format!("0{EAN}"));
    assert!(unknown["drug"].is_null());
    assert!(unknown["original"].is_null());

    let junk = scan(&app, "kein Code").await;
    assert!(junk["gtin"].is_null());
    assert!(junk["drug"].is_null());
}

/// Deleting an incomplete drug takes its codes along.
#[tokio::test]
async fn a_deleted_draft_takes_its_codes() {
    let db = common::database().await;
    let pool = db.pool();
    let app = TestApp::new(pool.clone()).await;
    let drug_id = app.post_empty("/api/drugs").await.id();
    app.post(
        &format!("/api/drugs/{drug_id}/codes"),
        json!({ "code": EAN }),
    )
    .await;
    assert_eq!(
        app.delete(&format!("/api/drugs/{drug_id}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert!(scan(&app, EAN).await["drug"].is_null());
}
