//! Google Maps: the kilometres a Wegegeld line is prefilled with — from the customer's previous
//! visit, else Google's shortest route — the mark an address change sets, recomputing a line,
//! and the address check. Google itself is `common::FakeGoogle`.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use common::{FakeGoogle, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn maps_app(pool: &PgPool, google: &FakeGoogle) -> TestApp {
    TestApp::with_config(pool.clone(), |config| google.configure(config)).await
}

/// A visit of `customer_id`, `days_ago` days back, with one of their animals on it.
async fn visit(pool: &PgPool, customer_id: i64, days_ago: i64) -> i64 {
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id =
        common::seed_appointment(pool, Utc::now() - Duration::days(days_ago)).await;
    common::seed_treatment(pool, appointment_id, patient_id).await
}

async fn add_line(app: &TestApp, treatment_id: i64, body: Value) -> Value {
    let response = app
        .post(&format!("/api/treatments/{treatment_id}/items"), body)
        .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    response.json()
}

async fn add_wegegeld(app: &TestApp, treatment_id: i64, service_id: i64) -> Value {
    add_line(
        app,
        treatment_id,
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await
}

async fn outdated(pool: &PgPool, customer_id: i64) -> bool {
    sqlx::query_scalar("SELECT travel_distance_outdated FROM customer WHERE id = $1")
        .bind(customer_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn starts_at(pool: &PgPool, treatment_id: i64) -> DateTime<Utc> {
    sqlx::query_scalar(
        "SELECT appointment.starts_at FROM treatment
         JOIN appointment ON appointment.id = treatment.appointment_id WHERE treatment.id = $1",
    )
    .bind(treatment_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn the_first_wegegeld_takes_googles_shortest_route(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;

    let line = add_wegegeld(&app, treatment_id, wegegeld).await;

    // The shorter of the two routes, 11.902 km, in whole kilometres and priced per GOT § 10:
    // 12 × 3.50.
    assert_eq!(line["km"], "12.00");
    assert_eq!(line["price_net"], "42.00");
    assert_eq!(line["km_suggestion"]["source"], "route");
    assert_eq!(line["km_suggestion"]["km"], "12");
    assert_eq!(line["km_suggestion"]["approximate"], false);

    let requests = google.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.path, "/directions/v2:computeRoutes");
    assert_eq!(request.api_key.as_deref(), Some("test-key"));
    assert!(
        request
            .field_mask
            .as_deref()
            .is_some_and(|mask| mask.contains("routes.distanceMeters"))
    );
    assert_eq!(
        request.body["origin"]["address"],
        "Praxisweg 1, 12345 Praxisstadt, Germany"
    );
    assert_eq!(
        request.body["destination"]["address"],
        "Musterweg 5, 12345 Musterstadt, Germany"
    );
    assert_eq!(request.body["travelMode"], "DRIVE");
    assert_eq!(request.body["routingPreference"], "TRAFFIC_UNAWARE");
    assert_eq!(
        request.body["requestedReferenceRoutes"],
        json!(["SHORTER_DISTANCE"])
    );
    // An address, never a name.
    assert!(!request.body.to_string().contains("Mustermann"));
}

#[sqlx::test]
async fn a_later_visit_takes_over_the_kilometres_the_vet_settled_on(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;

    let first = visit(&pool, customer_id, 30).await;
    let line = add_wegegeld(&app, first, wegegeld).await;
    // The vet knows the way better than Google.
    let corrected = app
        .patch(
            &format!("/api/treatment-items/{}", line["id"]),
            json!({ "km": "13.2" }),
        )
        .await;
    assert_eq!(corrected.status, StatusCode::OK);

    let second = visit(&pool, customer_id, 0).await;
    let line = add_wegegeld(&app, second, wegegeld).await;
    assert_eq!(line["km"], "13.20");
    assert_eq!(line["km_suggestion"]["source"], "previous");
    let from: DateTime<Utc> =
        serde_json::from_value(line["km_suggestion"]["from_visit"].clone()).unwrap();
    assert_eq!(from, starts_at(&pool, first).await);
    assert_eq!(google.requests().len(), 1, "Google is not asked again");

    // Another customer's kilometres are never someone else's.
    let neighbour = common::seed_customer(&pool).await;
    let theirs = visit(&pool, neighbour, 0).await;
    let line = add_wegegeld(&app, theirs, wegegeld).await;
    assert_eq!(line["km"], "12.00");
    assert_eq!(line["km_suggestion"]["source"], "route");
    assert_eq!(google.requests().len(), 2);
}

#[sqlx::test]
async fn nothing_is_looked_up_without_a_reason(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let house_call = common::seed_visit_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;

    // Kilometres given are kilometres taken.
    let line = add_line(
        &app,
        treatment_id,
        json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "7" }),
    )
    .await;
    assert_eq!(line["km"], "7.00");
    assert_eq!(line["km_suggestion"], Value::Null);

    // A house-call fee is per visit, but not Wegegeld.
    let line = add_line(
        &app,
        treatment_id,
        json!({ "kind": "service", "service_id": house_call, "quantity": "1" }),
    )
    .await;
    assert_eq!(line["km"], Value::Null);
    assert_eq!(line["km_suggestion"], Value::Null);

    assert!(google.requests().is_empty());
}

#[sqlx::test]
async fn without_a_key_the_field_stays_empty_as_before(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;

    let line = add_wegegeld(&app, treatment_id, wegegeld).await;
    assert_eq!(line["km"], Value::Null);
    assert_eq!(line["price_net"], "13.00");
    assert_eq!(line["km_suggestion"], Value::Null);

    let config = app.get("/api/config").await.json();
    assert_eq!(config["maps_enabled"], false);
}

#[sqlx::test]
async fn a_changed_home_address_asks_google_again(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;

    let first = visit(&pool, customer_id, 30).await;
    let old = add_line(
        &app,
        first,
        json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "25" }),
    )
    .await;

    // Contact details are not the way there.
    for change in [
        json!({ "phone": "+49 30 1234567" }),
        json!({ "invoice_street": "Postfach 12" }),
    ] {
        let response = app
            .patch(&format!("/api/customers/{customer_id}"), change)
            .await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(!outdated(&pool, customer_id).await);
    }

    let moved = app
        .patch(
            &format!("/api/customers/{customer_id}"),
            json!({ "home_street": "Neue Straße 1" }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK);
    assert!(outdated(&pool, customer_id).await);

    let second = visit(&pool, customer_id, 10).await;
    let line = add_wegegeld(&app, second, wegegeld).await;
    assert_eq!(line["km_suggestion"]["source"], "route");
    assert_eq!(line["km"], "12.00");
    assert_eq!(
        google.requests()[0].body["destination"]["address"],
        "Neue Straße 1, 12345 Musterstadt, Germany"
    );
    // The new distance is in: from now on the previous visit counts again.
    assert!(!outdated(&pool, customer_id).await);
    let third = visit(&pool, customer_id, 0).await;
    let line = add_wegegeld(&app, third, wegegeld).await;
    assert_eq!(line["km_suggestion"]["source"], "previous");
    assert_eq!(line["km"], "12.00");

    // What was billed before keeps its kilometres.
    let items = app
        .get(&format!("/api/treatments/{first}/items"))
        .await
        .json();
    assert_eq!(items[0]["id"], old["id"]);
    assert_eq!(items[0]["km"], "25.00");
}

#[sqlx::test]
async fn kilometres_typed_by_the_vet_clear_the_mark_too(pool: PgPool) {
    // No key: once marked, nothing is taken over, and the vet types the distance herself.
    let app = TestApp::new(pool.clone()).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let first = visit(&pool, customer_id, 30).await;
    add_line(
        &app,
        first,
        json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "25" }),
    )
    .await;
    app.patch(
        &format!("/api/customers/{customer_id}"),
        json!({ "home_zip": "54321" }),
    )
    .await;
    assert!(outdated(&pool, customer_id).await);

    let second = visit(&pool, customer_id, 10).await;
    let line = add_wegegeld(&app, second, wegegeld).await;
    assert_eq!(line["km"], Value::Null, "the old way there is not offered");
    assert_eq!(line["km_suggestion"], Value::Null);
    app.patch(
        &format!("/api/treatment-items/{}", line["id"]),
        json!({ "km": "20" }),
    )
    .await;
    assert!(!outdated(&pool, customer_id).await);

    let third = visit(&pool, customer_id, 0).await;
    let line = add_wegegeld(&app, third, wegegeld).await;
    assert_eq!(line["km"], "20.00");
    assert_eq!(line["km_suggestion"]["source"], "previous");
}

#[sqlx::test]
async fn a_practice_move_marks_every_customer(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    common::seed_practice_address(&pool).await;
    let first = common::seed_customer(&pool).await;
    let second = common::seed_customer(&pool).await;

    // The practice's name is not where it is.
    app.patch(
        "/api/settings",
        json!({ "practice_name": "Tierarztpraxis Neu" }),
    )
    .await;
    assert!(!outdated(&pool, first).await);

    let moved = app
        .patch("/api/settings", json!({ "practice_city": "Neustadt" }))
        .await;
    assert_eq!(moved.status, StatusCode::OK);
    assert!(outdated(&pool, first).await);
    assert!(outdated(&pool, second).await);
}

#[sqlx::test]
async fn a_copied_appointment_keeps_the_mark(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 30).await;
    add_line(
        &app,
        treatment_id,
        json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "25" }),
    )
    .await;
    app.patch(
        &format!("/api/customers/{customer_id}"),
        json!({ "home_city": "Neustadt" }),
    )
    .await;

    let appointment_id: i64 =
        sqlx::query_scalar("SELECT appointment_id FROM treatment WHERE id = $1")
            .bind(treatment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let copy = app
        .post(
            &format!("/api/appointments/{appointment_id}/duplicate"),
            json!({ "price_mode": "verbatim" }),
        )
        .await;
    assert_eq!(copy.status, StatusCode::OK);
    // The copy repeats the old kilometres; they are no new distance.
    assert!(outdated(&pool, customer_id).await);
}

#[sqlx::test]
async fn recomputing_asks_google_for_this_line(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    let line = add_line(
        &app,
        treatment_id,
        json!({
            "kind": "service", "service_id": wegegeld, "quantity": "1",
            "km": "30", "km_multiplier": "2"
        }),
    )
    .await;
    let item_id = line["id"].as_i64().unwrap();
    sqlx::query("UPDATE customer SET travel_distance_outdated = true WHERE id = $1")
        .bind(customer_id)
        .execute(&pool)
        .await
        .unwrap();

    // Google down: the line stays as it is, and says why.
    google.answer_routes(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": "backend" }),
    );
    let failed = app
        .post_empty(&format!("/api/treatment-items/{item_id}/recompute-km"))
        .await
        .json();
    assert_eq!(failed["km"], "30.00");
    assert_eq!(failed["km_suggestion"]["problem"], "unavailable");
    assert!(outdated(&pool, customer_id).await);

    google.answer_routes(StatusCode::OK, common::two_routes(12_437, 11_902));
    let recomputed = app
        .post_empty(&format!("/api/treatment-items/{item_id}/recompute-km"))
        .await;
    assert_eq!(recomputed.status, StatusCode::OK);
    let recomputed = recomputed.json();
    // The factor for adverse conditions stays: 12 × 3.50 × 2.
    assert_eq!(recomputed["km"], "12.00");
    assert_eq!(recomputed["km_multiplier"], "2.000");
    assert_eq!(recomputed["price_net"], "84.00");
    assert_eq!(recomputed["km_suggestion"]["source"], "route");
    assert!(!outdated(&pool, customer_id).await);

    // Only a Wegegeld line has a way to recompute.
    let house_call = common::seed_visit_service(&pool).await;
    let other = add_line(
        &app,
        treatment_id,
        json!({ "kind": "service", "service_id": house_call, "quantity": "1" }),
    )
    .await;
    let refused = app
        .post_empty(&format!(
            "/api/treatment-items/{}/recompute-km",
            other["id"]
        ))
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Released, the line is frozen, and Google is not even asked.
    let invoice = app
        .post(
            &format!("/api/treatments/{treatment_id}/invoice"),
            json!({ "includes_finding": true }),
        )
        .await;
    assert_eq!(
        invoice.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&invoice.body)
    );
    let invoice_id = invoice.json()["id"].as_i64().unwrap();
    let accepted = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": [], "send_at": Utc::now(), "template": "Standard" }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    let asked = google.requests().len();
    let frozen = app
        .post_empty(&format!("/api/treatment-items/{item_id}/recompute-km"))
        .await;
    assert_eq!(frozen.status, StatusCode::CONFLICT);
    assert_eq!(google.requests().len(), asked);
}

#[sqlx::test]
async fn recomputing_needs_a_key(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    let line = add_line(
        &app,
        treatment_id,
        json!({ "kind": "service", "service_id": wegegeld, "quantity": "1", "km": "9" }),
    )
    .await;

    let refused = app
        .post_empty(&format!("/api/treatment-items/{}/recompute-km", line["id"]))
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.error_fields(), vec!["km"]);
}

#[sqlx::test]
async fn a_line_without_kilometres_says_why(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    let wegegeld = common::seed_travel_service(&pool).await;

    // No practice address yet: nowhere to drive from.
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    let line = add_wegegeld(&app, treatment_id, wegegeld).await;
    assert_eq!(line["km"], Value::Null);
    assert_eq!(line["km_suggestion"]["problem"], "no_practice_address");

    common::seed_practice_address(&pool).await;
    let cases = [
        (StatusCode::OK, json!({}), "not_found"),
        (
            StatusCode::FORBIDDEN,
            json!({ "error": { "status": "PERMISSION_DENIED" } }),
            "unavailable",
        ),
        (
            StatusCode::OK,
            json!("not what Google sends"),
            "unavailable",
        ),
    ];
    for (status, answer, problem) in cases {
        google.answer_routes(status, answer);
        let customer_id = common::seed_customer(&pool).await;
        let treatment_id = visit(&pool, customer_id, 0).await;
        let line = add_wegegeld(&app, treatment_id, wegegeld).await;
        assert_eq!(line["km"], Value::Null, "{problem}");
        assert_eq!(line["price_net"], "13.00", "{problem}");
        assert_eq!(line["km_suggestion"]["problem"], problem);
    }

    // A customer with no home street has nowhere to drive to — and Google is not asked.
    let asked = google.requests().len();
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    sqlx::query("UPDATE customer SET draft = true, home_street = NULL WHERE id = $1")
        .bind(customer_id)
        .execute(&pool)
        .await
        .unwrap();
    let line = add_wegegeld(&app, treatment_id, wegegeld).await;
    assert_eq!(line["km_suggestion"]["problem"], "no_customer_address");
    assert_eq!(google.requests().len(), asked);
}

#[sqlx::test]
async fn an_address_google_places_only_roughly_is_flagged(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    google.answer_routes(
        StatusCode::OK,
        json!({
            "routes": [{ "distanceMeters": 8000 }],
            "geocodingResults": { "destination": { "partialMatch": true, "type": ["locality"] } },
        }),
    );

    let line = add_wegegeld(&app, treatment_id, wegegeld).await;
    assert_eq!(line["km"], "8.00");
    assert_eq!(line["km_suggestion"]["approximate"], true);
}

#[sqlx::test]
async fn a_treatment_group_with_wegegeld_gets_the_kilometres_on_that_line_only(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    common::seed_practice_address(&pool).await;
    let wegegeld = common::seed_travel_service(&pool).await;
    let house_call = common::seed_visit_service(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let treatment_id = visit(&pool, customer_id, 0).await;
    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO treatment_template (name, draft) VALUES ('Hausbesuch', false) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for (position, service_id) in [(1, house_call), (2, wegegeld)] {
        sqlx::query(
            "INSERT INTO treatment_template_item (template_id, position, kind, service_id, quantity)
             VALUES ($1, $2, 'service', $3, 1)",
        )
        .bind(template_id)
        .bind(position)
        .bind(service_id)
        .execute(&pool)
        .await
        .unwrap();
    }

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
    let by_name = |name: &str| {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(by_name("Wegegeld")["km"], "12.00");
    assert_eq!(by_name("Hausbesuch")["km"], Value::Null);
    assert_eq!(google.requests().len(), 1);
}

#[sqlx::test]
async fn an_address_is_checked_as_typed(pool: PgPool) {
    let google = FakeGoogle::start().await;
    let app = maps_app(&pool, &google).await;
    let check = |street: &str| json!({ "street": street, "zip": "12345", "city": "Musterstadt", "country": "DE" });

    let found = app.post("/api/address-check", check("Musterweg 5")).await;
    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(
        found.json(),
        json!({ "verdict": "found", "suggestion": null })
    );
    let request = google.requests().pop().unwrap();
    assert_eq!(request.path, "/v1:validateAddress");
    assert_eq!(request.api_key.as_deref(), Some("test-key"));
    assert_eq!(
        request.body,
        json!({ "address": {
            "regionCode": "DE", "postalCode": "12345", "locality": "Musterstadt",
            "addressLines": ["Musterweg 5"],
        } })
    );

    google.answer_address(
        StatusCode::OK,
        common::address_answer("PREMISE", "CONFIRMED", true, "Musterweg 5"),
    );
    let corrected = app
        .post("/api/address-check", check("Musterwg 5"))
        .await
        .json();
    assert_eq!(corrected["verdict"], "corrected");
    assert_eq!(
        corrected["suggestion"],
        json!({ "street": "Musterweg 5", "zip": "12345", "city": "Musterstadt" })
    );

    google.answer_address(
        StatusCode::OK,
        common::address_answer("ROUTE", "UNCONFIRMED_BUT_PLAUSIBLE", false, "Musterweg 99"),
    );
    let unknown = app
        .post("/api/address-check", check("Musterweg 99"))
        .await
        .json();
    assert_eq!(unknown["verdict"], "not_found");

    google.answer_address(StatusCode::INTERNAL_SERVER_ERROR, json!({}));
    let down = app
        .post("/api/address-check", check("Musterweg 5"))
        .await
        .json();
    assert_eq!(down["verdict"], "unavailable");

    // Half an address is not asked about.
    let asked = google.requests().len();
    let partial = app
        .post(
            "/api/address-check",
            json!({ "street": "Musterweg 5", "zip": "", "city": "" }),
        )
        .await
        .json();
    assert_eq!(partial["verdict"], "unavailable");
    assert_eq!(google.requests().len(), asked);

    let config = app.get("/api/config").await.json();
    assert_eq!(config["maps_enabled"], true);
}

#[sqlx::test]
async fn the_address_check_needs_a_key(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let refused = app
        .post(
            "/api/address-check",
            json!({ "street": "Musterweg 5", "zip": "12345", "city": "Musterstadt" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
}
