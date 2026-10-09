//! The GOT 2022 catalogue as migrations `0008` and `0021` leave it (FR-022).
//!
//! `0008` imported the fee schedule with an extraction that lost 75 positions, read five
//! numbers out of their own descriptions, one fee from the wrong place, swallowed suspended
//! hyphens and hid by the wrong chapters. `0021` corrects that; these tests hold the result
//! against the published schedule, and show that the correction leaves the vet's own edits
//! alone.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use rust_decimal::Decimal;
use sqlx::PgPool;

/// The last migration before the correction.
const BEFORE_CORRECTION: i64 = 20;
const CORRECTION: i64 = 21;

async fn position(pool: &PgPool, number: &str) -> (String, Decimal, bool) {
    sqlx::query_as(
        "SELECT name, net_price, hidden FROM service WHERE type = 'got' AND got_number = $1",
    )
    .bind(number)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|error| panic!("GOT {number} is in the catalogue: {error}"))
}

#[sqlx::test]
async fn every_position_of_the_schedule_is_there_exactly_once(pool: PgPool) {
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM service WHERE type = 'got'")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(total, 1006, "the GOT 2022 schedule has 1006 positions");

    let missing: Vec<i32> = sqlx::query_scalar(
        "SELECT number FROM generate_series(1, 1006) AS number
          WHERE NOT EXISTS (SELECT 1 FROM service WHERE type = 'got' AND got_number = number::text)",
    )
    .fetch_all(&pool)
    .await
    .expect("gaps");
    assert_eq!(
        missing,
        Vec::<i32>::new(),
        "no number between 1 and 1006 is missing"
    );

    let foreign: Vec<String> = sqlx::query_scalar(
        "SELECT got_number FROM service
          WHERE type = 'got' AND NOT (got_number ~ '^[0-9]+$' AND got_number::int BETWEEN 1 AND 1006)",
    )
    .fetch_all(&pool)
    .await
    .expect("foreign numbers");
    assert_eq!(
        foreign,
        Vec::<String>::new(),
        "no number outside the schedule"
    );
}

#[sqlx::test]
async fn positions_carry_the_published_number_text_and_fee(pool: PgPool) {
    // (number, start of the published text, net fee) — each one damaged by the first import.
    let published = [
        // Its number had been read from "101 bis zu 150 Tieren".
        (
            "46",
            "Bestandsuntersuchung Kalb, 101 bis zu 150 Tieren",
            "51.13",
        ),
        (
            "63",
            "Bestandsuntersuchung Nutzgeflügel, 2001 bis zu 3.000 Tieren",
            "59.54",
        ),
        // The number those rows had taken, now holding its own position.
        ("101", "Punktion, Abszess, Zyste", "15.39"),
        // Missing: fees of 333,34 € and more had a three-fold rate the parser could not read.
        ("175", "CT-Untersuchung eines Körperteils", "350.00"),
        ("481", "Darmresektion einschließlich Laparotomie", "1350.00"),
        // Missing as well: its number collided with a misread one.
        (
            "50",
            "Bestandsuntersuchung kleine Hauswiederkäuer, 151 bis zu 500 Tieren",
            "50.38",
        ),
        // Present, with a fee read from the wrong place (23,51 €).
        (
            "193",
            "Bestrahlungstherapie mittels Linearbeschleuniger",
            "219.42",
        ),
    ];
    for (number, text, fee) in published {
        let (name, net_price, _) = position(&pool, number).await;
        assert!(
            name.starts_with(text),
            "GOT {number}: {name:?} should start with {text:?}"
        );
        assert_eq!(
            net_price.to_string(),
            fee,
            "GOT {number} keeps the published net fee"
        );
    }
}

#[sqlx::test]
async fn suspended_hyphens_survive_in_names(pool: PgPool) {
    let glued: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM service WHERE type = 'got'
            AND name ~ '(Zucht|Lege|Rasse|Sehnen|Planungs|Thrombozyten|Unterkiefer)(und|oder|u\\.)'",
    )
    .fetch_all(&pool)
    .await
    .expect("names");
    assert_eq!(
        glued,
        Vec::<String>::new(),
        "\"Zucht- und\", never \"Zuchtund\""
    );

    let (name, _, _) = position(&pool, "894").await;
    assert_eq!(name, "Sehnen- und Muskelnaht");
}

#[sqlx::test]
async fn surgery_is_hidden_and_everything_else_is_not(pool: PgPool) {
    // Everyday work the first import hid, because "Nicht chirurgische Behandlungen" matched its
    // test for surgical chapters.
    for number in ["404", "455", "627", "716", "763", "985"] {
        let (name, _, hidden) = position(&pool, number).await;
        assert!(
            !hidden,
            "GOT {number} ({name}) is not surgery and must be offered"
        );
    }
    // Operative positions stay out of the pickers (logic.md) — including ones the first import
    // left visible, and ones it had not imported at all.
    for number in ["874", "951", "1005", "743"] {
        let (name, _, hidden) = position(&pool, number).await;
        assert!(hidden, "GOT {number} ({name}) is surgery and stays hidden");
    }
}

/// The correction runs against a catalogue the vet may already have worked with: an edit made
/// through the application survives it, and nothing she has used disappears from her pickers.
#[sqlx::test(migrations = false)]
async fn the_correction_leaves_the_vets_own_changes_alone(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(BEFORE_CORRECTION, &pool)
        .await
        .expect("migrate up to the correction");

    // She renamed a position…
    sqlx::query("UPDATE service SET name = 'Bestrahlung, palliativ' WHERE type = 'got' AND got_number = '193'")
        .execute(&pool)
        .await
        .expect("rename 193");
    // …and billed a surgical one the first import had left visible.
    let customer_id = common::seed_customer(&pool).await;
    let patient_id = common::seed_patient(&pool, customer_id).await;
    let appointment_id = common::seed_appointment(&pool, chrono::Utc::now()).await;
    let treatment_id = common::seed_treatment(&pool, appointment_id, patient_id).await;
    sqlx::query(
        "INSERT INTO treatment_item
             (treatment_id, position, kind, service_id, name, quantity, price_net, vat_percent,
              factor, got_number)
         SELECT $1, 1, 'service', id, name, 1, net_price, vat_percent, factor, got_number
           FROM service WHERE type = 'got' AND got_number = '951'",
    )
    .bind(treatment_id)
    .execute(&pool)
    .await
    .expect("bill 951");

    migrator
        .run_to(CORRECTION, &pool)
        .await
        .expect("apply the correction");

    let (name, net_price, _) = position(&pool, "193").await;
    assert_eq!(name, "Bestrahlung, palliativ", "her name stays");
    assert_eq!(
        net_price.to_string(),
        "219.42",
        "the wrong fee is still corrected"
    );

    let (_, _, hidden) = position(&pool, "951").await;
    assert!(!hidden, "a position she bills stays in her pickers");
    let (_, _, hidden) = position(&pool, "952").await;
    assert!(hidden, "an unused surgical position is hidden");
}
