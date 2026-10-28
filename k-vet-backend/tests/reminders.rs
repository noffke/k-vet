//! "Mitbringen" (migration 0035): drugs to bring to a visit, taken into a position on the day,
//! and the day's Packliste.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Local, TimeDelta, Utc};
use common::TestApp;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;

fn dec(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("test literal is a decimal")
}

/// Noon of the server's today, `days` later: the same calendar day whatever the hour now.
fn noon(days: i64) -> DateTime<Utc> {
    (Local::now().date_naive() + TimeDelta::days(days))
        .and_hms_opt(12, 0, 0)
        .and_then(|noon| noon.and_local_timezone(Local).earliest())
        .expect("noon")
        .with_timezone(&Utc)
}

/// An appointment at `starts_at` with one animal on it; returns the appointment, its treatment
/// and the animal's record.
async fn visit(pool: &PgPool, starts_at: DateTime<Utc>) -> (i64, i64, i64) {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, starts_at).await;
    let (treatment_id, record_id) =
        common::seed_patient_treatment(pool, appointment_id, patient_id).await;
    (appointment_id, treatment_id, record_id)
}

async fn remind(app: &TestApp, appointment_id: i64, packaging_id: i64, quantity: &str) -> Value {
    let created = app
        .post(
            &format!("/api/appointments/{appointment_id}/reminders"),
            json!({ "drug_packaging_id": packaging_id, "quantity": quantity }),
        )
        .await;
    assert_eq!(
        created.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    created.json()
}

/// A reminder is written as the vet picks it, changed and ticked off in place, and removed;
/// the positions close up. Nothing is dispensed by a reminder.
#[sqlx::test]
async fn a_reminder_is_kept_changed_and_removed(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let (appointment_id, ..) = visit(&pool, noon(1)).await;

    let bottle = remind(&app, appointment_id, drug.packaging_id, "1").await;
    let subsets = remind(&app, appointment_id, drug.subset_packaging_id, "3").await;
    assert_eq!(bottle["name"], "Amoxicillin 100");
    assert_eq!(bottle["position"], 1);
    assert_eq!(subsets["position"], 2);
    assert_eq!(subsets["size"], "10.00");
    assert_eq!(subsets["unit"], "ml");
    assert!(subsets["taken_item_id"].is_null());
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("100"));

    let changed = app
        .patch(
            &format!("/api/reminders/{}", subsets["id"]),
            json!({ "quantity": "2", "note": "  für Bello  ", "packed": true }),
        )
        .await
        .json();
    assert_eq!(changed["quantity"], "2.00");
    assert_eq!(changed["note"], "für Bello");
    assert_eq!(changed["packed"], true);
    let zero = app
        .patch(
            &format!("/api/reminders/{}", subsets["id"]),
            json!({ "quantity": "0" }),
        )
        .await;
    assert_eq!(zero.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(zero.error_fields(), vec!["quantity"]);

    // The appointment counts what is still to bring.
    let appointment = app
        .get(&format!("/api/appointments/{appointment_id}"))
        .await
        .json();
    assert_eq!(appointment["reminder_count"], 2);

    let removed = app
        .delete(&format!("/api/reminders/{}", bottle["id"]))
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let left = app
        .get(&format!("/api/appointments/{appointment_id}/reminders"))
        .await
        .json();
    assert_eq!(left.as_array().map(Vec::len), Some(1));
    assert_eq!(left[0]["position"], 1);

    // Only what the picker offers can be reminded of.
    sqlx::query("UPDATE drug_packaging SET archived = true WHERE id = $1")
        .bind(drug.packaging_id)
        .execute(&pool)
        .await
        .expect("archive the bottle");
    let archived = app
        .post(
            &format!("/api/appointments/{appointment_id}/reminders"),
            json!({ "drug_packaging_id": drug.packaging_id }),
        )
        .await;
    assert_eq!(archived.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        archived.json()["errors"][0]["message"],
        "reminder.drugUnavailable"
    );
}

/// On the day the reminder becomes a position: on the visit's animal, dispensed from the lots
/// as if picked. Once only; beyond the stock it is refused and stays a reminder.
#[sqlx::test]
async fn a_reminder_is_taken_into_a_position(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    let lot = common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let (appointment_id, treatment_id, record_id) = visit(&pool, noon(0)).await;
    let reminder = remind(&app, appointment_id, drug.subset_packaging_id, "3").await;
    let too_much = remind(&app, appointment_id, drug.subset_packaging_id, "8").await;
    assert_eq!(too_much["stock_short"], false, "80 ml of 100 are there");

    let taken = app
        .post(
            &format!("/api/reminders/{}/take", reminder["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        taken.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&taken.body)
    );
    let taken = taken.json();
    let lines = app
        .get(&format!("/api/treatments/{treatment_id}/items"))
        .await
        .json();
    assert_eq!(lines.as_array().map(Vec::len), Some(1));
    assert_eq!(taken["taken_item_id"], lines[0]["id"]);
    assert_eq!(lines[0]["patient_treatment_id"], record_id);
    assert_eq!(lines[0]["quantity"], "3.00");
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("70"));

    let again = app
        .post(
            &format!("/api/reminders/{}/take", reminder["id"]),
            json!({}),
        )
        .await;
    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        again.json()["errors"][0]["message"],
        "reminder.alreadyTaken"
    );

    // 80 ml are no longer there: refused with the picker's reason, nothing written.
    let reminders = app
        .get(&format!("/api/appointments/{appointment_id}/reminders"))
        .await
        .json();
    assert_eq!(reminders[1]["stock_short"], true);
    let short = app
        .post(
            &format!("/api/reminders/{}/take", too_much["id"]),
            json!({ "patient_treatment_id": record_id }),
        )
        .await;
    assert_eq!(short.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(short.json()["errors"][0]["message"], "item.stockShort");
    let after = app
        .get(&format!("/api/appointments/{appointment_id}/reminders"))
        .await
        .json();
    assert!(after[1]["taken_item_id"].is_null());
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("70"));

    // The position removed again, the reminder is one again.
    app.delete(&format!("/api/treatment-items/{}", lines[0]["id"]))
        .await;
    let back = app
        .get(&format!("/api/appointments/{appointment_id}/reminders"))
        .await
        .json();
    assert!(back[0]["taken_item_id"].is_null());
    assert_eq!(common::lot_remaining(&pool, lot).await, dec("100"));
}

/// Released, the invoice holds the treatment: a reminder cannot become a position any more.
#[sqlx::test]
async fn a_released_invoice_takes_no_reminder(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let service_id = common::seed_got_service(&pool).await;
    let (appointment_id, treatment_id, _) = visit(&pool, noon(0)).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({}),
        )
        .await
        .json();
    app.post(
        &format!("/api/invoices/{}/accept", invoice["id"]),
        json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
    )
    .await;
    let reminder = remind(&app, appointment_id, drug.subset_packaging_id, "1").await;

    let refused = app
        .post(
            &format!("/api/reminders/{}/take", reminder["id"]),
            json!({}),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
}

/// The Packliste adds up a day's open reminders per packaging and checks the shelf per drug —
/// a bottle and its subsets draw from the same lots — and lists them visit by visit, in time
/// order. Other days and reminders already taken are not on it.
#[sqlx::test]
async fn the_packing_list_adds_up_the_day(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let drug = common::seed_drug(&pool).await;
    common::seed_lot(&pool, drug.packaging_id, 1, dec("100"), None).await;
    let (late, ..) = visit(&pool, noon(0) + TimeDelta::hours(3)).await;
    let (early, ..) = visit(&pool, noon(0) - TimeDelta::hours(2)).await;
    let (tomorrow, ..) = visit(&pool, noon(1)).await;

    remind(&app, late, drug.subset_packaging_id, "4").await;
    remind(&app, early, drug.subset_packaging_id, "3").await;
    remind(&app, early, drug.packaging_id, "1").await;
    remind(&app, tomorrow, drug.packaging_id, "5").await;
    let taken = remind(&app, late, drug.subset_packaging_id, "1").await;
    app.post(&format!("/api/reminders/{}/take", taken["id"]), json!({}))
        .await;

    let today = Local::now().date_naive();
    let list = app
        .get(&format!("/api/packing-list?date={today}"))
        .await
        .json();
    assert_eq!(list["date"], today.to_string());

    // 7 subsets (70 ml) and a bottle (100 ml) need 170 ml; 90 are left after the one taken.
    let totals = list["totals"].as_array().expect("totals");
    assert_eq!(totals.len(), 2, "{totals:?}");
    let subset_total = totals
        .iter()
        .find(|total| total["drug_packaging_id"] == drug.subset_packaging_id)
        .expect("the subsets");
    assert_eq!(subset_total["quantity"], "7.00");
    assert_eq!(subset_total["in_stock"], "90.00");
    assert_eq!(subset_total["short"], true);
    let bottle_total = totals
        .iter()
        .find(|total| total["drug_packaging_id"] == drug.packaging_id)
        .expect("the bottle");
    assert_eq!(bottle_total["quantity"], "1.00");
    assert_eq!(
        bottle_total["short"], true,
        "the same lots, counted together"
    );

    let visits = list["appointments"].as_array().expect("appointments");
    let order: Vec<i64> = visits
        .iter()
        .map(|visit| visit["appointment_id"].as_i64().unwrap())
        .collect();
    assert_eq!(order, [early, late]);
    assert_eq!(visits[0]["reminders"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        visits[1]["reminders"].as_array().map(Vec::len),
        Some(1),
        "the taken one is not on it"
    );
    assert!(!visits[0]["patients"].as_array().unwrap().is_empty());

    // Tomorrow's own list holds only tomorrow's five bottles, more than the shelf has.
    let tomorrow_list = app
        .get(&format!(
            "/api/packing-list?date={}",
            today + TimeDelta::days(1)
        ))
        .await
        .json();
    assert_eq!(tomorrow_list["totals"][0]["quantity"], "5.00");
    assert_eq!(tomorrow_list["totals"][0]["short"], true, "500 ml of 90");
}
