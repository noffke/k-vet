//! The appointments as a calendar on the phone (CalDAV at `/dav`, migration 0036): the
//! discovery a sync app walks, the events it reads, and what it may write back.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::{HeaderName, StatusCode, header};
use chrono::{DateTime, TimeDelta, Utc};
use common::{TestApp, TestResponse};
use serde_json::json;
use sqlx::PgPool;

async fn app(pool: &PgPool) -> TestApp {
    TestApp::with_config(pool.clone(), |config| {
        config.dav.enabled = true;
        config.server.base_url = "http://pi:8080".to_owned();
    })
    .await
}

fn text(response: &TestResponse) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}

/// iCalendar text with its long lines joined again (RFC 5545 folds them at 75 octets).
fn unfolded(ics: &str) -> String {
    ics.replace("\r\n ", "")
}

const PROPFIND_ALL: &str = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"
    xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/"><d:prop>
    <d:resourcetype/><d:displayname/><d:current-user-principal/><c:calendar-home-set/>
    <c:supported-calendar-component-set/><cs:getctag/><d:current-user-privilege-set/>
    <d:getetag/><d:sync-token/></d:prop></d:propfind>"#;

/// `<d:getetag>"…"</d:getetag>` of `href` in a multistatus.
fn etag_in(multistatus: &str, href: &str) -> Option<String> {
    let document = roxmltree::Document::parse(multistatus).expect("a multistatus");
    document
        .descendants()
        .filter(|node| node.tag_name().name() == "response")
        .find(|response| {
            response
                .descendants()
                .any(|node| node.tag_name().name() == "href" && node.text() == Some(href))
        })
        .and_then(|response| {
            response
                .descendants()
                .find(|node| node.tag_name().name() == "getetag")
                .and_then(|node| node.text())
                .map(str::to_owned)
        })
}

fn ctag_in(multistatus: &str) -> String {
    let document = roxmltree::Document::parse(multistatus).expect("a multistatus");
    document
        .descendants()
        .find(|node| node.tag_name().name() == "getctag")
        .and_then(|node| node.text())
        .expect("a ctag")
        .to_owned()
}

async fn listing(app: &TestApp) -> String {
    let response = app
        .dav(
            "PROPFIND",
            "/dav/calendars/praxis/",
            &[("Depth", "1")],
            PROPFIND_ALL,
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::MULTI_STATUS,
        "{}",
        text(&response)
    );
    text(&response)
}

/// An appointment at `starts_at` with Erika Mustermann's Bello (Hund) on it.
async fn visit(pool: &PgPool, starts_at: DateTime<Utc>) -> (i64, i64) {
    let customer_id = common::seed_customer(pool).await;
    sqlx::query("UPDATE customer SET phone = '030 1234567' WHERE id = $1")
        .bind(customer_id)
        .execute(pool)
        .await
        .expect("a phone number");
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, starts_at).await;
    let (treatment_id, _) = common::seed_patient_treatment(pool, appointment_id, patient_id).await;
    (appointment_id, treatment_id)
}

fn phone_event(uid: &str, start: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:DAVx5\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20261020T080000Z\r\n{start}\r\nDTEND:20261030T090000Z\r\n\
         SUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// What a sync app walks on first setup: the well-known address, the principal, its calendar
/// home, the one calendar in it — signed in with the k-vet login.
#[sqlx::test]
async fn the_sync_app_finds_the_calendar(pool: PgPool) {
    let app = app(&pool).await;

    let well_known = app.dav("GET", "/.well-known/caldav", &[], "").await;
    assert!(well_known.status.is_redirection());
    assert_eq!(well_known.header(header::LOCATION), Some("/dav/"));

    let root = app
        .dav("PROPFIND", "/dav/", &[("Depth", "0")], PROPFIND_ALL)
        .await;
    assert_eq!(root.status, StatusCode::MULTI_STATUS);
    assert!(
        text(&root)
            .contains("<d:current-user-principal><d:href>/dav/principals/tierarzt/</d:href>")
    );

    let principal = app
        .dav(
            "PROPFIND",
            "/dav/principals/tierarzt/",
            &[("Depth", "0")],
            PROPFIND_ALL,
        )
        .await;
    assert!(
        text(&principal).contains("<c:calendar-home-set><d:href>/dav/calendars/</d:href>"),
        "{}",
        text(&principal)
    );

    let home = app
        .dav(
            "PROPFIND",
            "/dav/calendars/",
            &[("Depth", "1")],
            PROPFIND_ALL,
        )
        .await;
    let home = text(&home);
    assert!(
        home.contains("<d:href>/dav/calendars/praxis/</d:href>"),
        "{home}"
    );
    assert!(home.contains("<d:displayname>Praxis-Termine</d:displayname>"));
    assert!(home.contains(r#"<c:comp name="VEVENT"/>"#));
    assert!(home.contains("<d:privilege><d:write/></d:privilege>"));
    assert!(home.contains("<cs:getctag>"));
    // No sync-token: the client syncs by ctag and ETags.
    assert!(home.contains("<d:sync-token/>"));

    let options = app
        .dav(
            "OPTIONS",
            "/dav/calendars/praxis/",
            &[("Authorization", "")],
            "",
        )
        .await;
    assert_eq!(options.status, StatusCode::OK);
    assert!(
        options
            .header(HeaderName::from_static("dav"))
            .unwrap_or_default()
            .contains("calendar-access")
    );
}

/// The phone signs in with the k-vet login on every request; anything else is turned away,
/// and guessing runs into the login's throttle. Switched off, there is no calendar.
#[sqlx::test]
async fn only_the_k_vet_login_opens_the_calendar(pool: PgPool) {
    let app = app(&pool).await;
    let anonymous = app
        .dav("PROPFIND", "/dav/", &[("Authorization", "")], "")
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert!(
        anonymous
            .header(header::WWW_AUTHENTICATE)
            .unwrap_or_default()
            .starts_with("Basic")
    );
    // "tierarzt:falsch"
    let wrong = [("Authorization", "Basic dGllcmFyenQ6ZmFsc2No")];
    // The login form's throttle: ten failures a minute.
    for _ in 0..10 {
        assert_eq!(
            app.dav("PROPFIND", "/dav/", &wrong, "").await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        app.dav("PROPFIND", "/dav/", &wrong, "").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    let off = TestApp::new(pool.clone()).await;
    assert_ne!(
        off.dav("PROPFIND", "/dav/", &[], "").await.status,
        StatusCode::MULTI_STATUS
    );
}

/// Each appointment from the window on is an event that says whose visit it is, where, and
/// what to bring; a change to any of that is a new ETag and a new ctag, so the phone fetches it.
#[sqlx::test]
async fn appointments_are_events(pool: PgPool) {
    let app = app(&pool).await;
    let tomorrow = Utc::now() + TimeDelta::days(1);
    let (appointment_id, _) = visit(&pool, tomorrow).await;
    let (long_ago, _) = visit(&pool, Utc::now() - TimeDelta::days(200)).await;
    let drug = common::seed_drug(&pool).await;
    app.post(
        &format!("/api/appointments/{appointment_id}/reminders"),
        json!({ "drug_packaging_id": drug.subset_packaging_id, "quantity": "2" }),
    )
    .await;

    let href = format!("/dav/calendars/praxis/kvet-{appointment_id}.ics");
    let before = listing(&app).await;
    let etag = etag_in(&before, &href).expect("the appointment is listed");
    assert!(
        etag_in(
            &before,
            &format!("/dav/calendars/praxis/kvet-{long_ago}.ics")
        )
        .is_none(),
        "200 days back is outside the window"
    );

    let event = app.dav("GET", &href, &[], "").await;
    assert_eq!(event.status, StatusCode::OK);
    assert_eq!(event.header(header::ETAG), Some(etag.as_str()));
    let ics = unfolded(&text(&event));
    assert!(
        ics.contains("SUMMARY:Erika Mustermann – Bello (Hund)"),
        "{ics}"
    );
    assert!(
        ics.contains("LOCATION:Musterweg 5\\, 12345 Musterstadt"),
        "{ics}"
    );
    assert!(ics.contains("Tel. 030 1234567"), "{ics}");
    assert!(ics.contains("2× Amoxicillin 100 · 10 ml"), "{ics}");
    assert!(ics.contains(&format!("http://pi:8080/appointments/{appointment_id}")));

    let multiget = format!(
        r#"<c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
           <d:prop><d:getetag/><c:calendar-data/></d:prop><d:href>{href}</d:href>
           <d:href>/dav/calendars/praxis/gone.ics</d:href></c:calendar-multiget>"#
    );
    let report = app
        .dav(
            "REPORT",
            "/dav/calendars/praxis/",
            &[("Depth", "1")],
            &multiget,
        )
        .await;
    let report = text(&report);
    assert!(report.contains("BEGIN:VEVENT"), "{report}");
    assert!(report.contains("HTTP/1.1 404 Not Found"));

    // A new phone number is a new event text, a new ETag and a new ctag.
    sqlx::query("UPDATE customer SET phone = '0171 7654321'")
        .execute(&pool)
        .await
        .expect("a new number");
    let after = listing(&app).await;
    assert_ne!(etag_in(&after, &href), Some(etag));
    assert_ne!(ctag_in(&after), ctag_in(&before));
}

/// An entry made on the phone becomes an appointment without a customer; its title names the
/// customer to suggest. The answer carries no ETag: what k-vet keeps is not what was sent.
#[sqlx::test]
async fn an_entry_made_on_the_phone_becomes_an_appointment(pool: PgPool) {
    let app = app(&pool).await;
    let (_, _) = visit(&pool, Utc::now() + TimeDelta::days(3)).await;
    let body = phone_event(
        "abc-123",
        "DTSTART;TZID=Europe/Berlin:20261030T080000",
        "Mustermann Bello impfen",
    );
    let created = app
        .dav(
            "PUT",
            "/dav/calendars/praxis/abc-123.ics",
            &[("If-None-Match", "*"), ("Content-Type", "text/calendar")],
            &body,
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", text(&created));
    assert!(created.header(header::ETAG).is_none());

    let appointment = sqlx::query_as::<_, (i64, DateTime<Utc>, Option<i64>, String)>(
        "SELECT id, starts_at, customer_id, calendar_origin FROM appointment
          WHERE calendar_href = 'abc-123.ics'",
    )
    .fetch_one(&pool)
    .await
    .expect("an appointment");
    assert_eq!(
        appointment.1,
        "2026-10-30T07:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    assert_eq!(appointment.2, None);
    assert_eq!(appointment.3, "phone");
    let shown = app
        .get(&format!("/api/appointments/{}", appointment.0))
        .await
        .json();
    assert_eq!(shown["from_phone"], true);
    assert_eq!(shown["calendar_title"], "Mustermann Bello impfen");

    // The title names Mustermann and Bello: that customer is suggested, never assigned.
    let suggestions = app
        .get(&format!(
            "/api/appointments/{}/customer-suggestions",
            appointment.0
        ))
        .await
        .json();
    assert_eq!(suggestions[0]["name"], "Erika Mustermann");
    assert_eq!(suggestions[0]["patients"], json!(["Bello"]));

    // Served back under the phone's own name and UID, its title kept.
    let event = text(
        &app.dav("GET", "/dav/calendars/praxis/abc-123.ics", &[], "")
            .await,
    );
    assert!(event.contains("UID:abc-123"), "{event}");
    assert!(event.contains("SUMMARY:Mustermann Bello impfen"));

    let again = app
        .dav(
            "PUT",
            "/dav/calendars/praxis/abc-123.ics",
            &[("If-None-Match", "*")],
            &body,
        )
        .await;
    assert_eq!(again.status, StatusCode::PRECONDITION_FAILED);
}

/// Moved on the phone, the appointment moves — unless its invoice already bills the date;
/// then k-vet's time stands and the phone fetches it back. A stale ETag is refused.
#[sqlx::test]
async fn a_moved_event_moves_its_appointment_unless_invoiced(pool: PgPool) {
    let app = app(&pool).await;
    let (appointment_id, treatment_id) = visit(&pool, Utc::now() + TimeDelta::days(2)).await;
    let href = format!("/dav/calendars/praxis/kvet-{appointment_id}.ics");
    let etag = app
        .dav("GET", &href, &[], "")
        .await
        .header(header::ETAG)
        .unwrap()
        .to_owned();
    let moved = phone_event(
        &format!("kvet-{appointment_id}"),
        "DTSTART:20261105T090000Z",
        "egal",
    );

    let stale = app
        .dav("PUT", &href, &[("If-Match", "\"veraltet\"")], &moved)
        .await;
    assert_eq!(stale.status, StatusCode::PRECONDITION_FAILED);

    let taken = app.dav("PUT", &href, &[("If-Match", &etag)], &moved).await;
    assert_eq!(taken.status, StatusCode::NO_CONTENT);
    let starts_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT starts_at FROM appointment WHERE id = $1")
            .bind(appointment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        starts_at,
        "2026-11-05T09:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    // The generated title stays; the phone's "egal" is not read back.
    let event = text(&app.dav("GET", &href, &[], "").await);
    assert!(
        event.contains("SUMMARY:Erika Mustermann – Bello (Hund)"),
        "{event}"
    );

    // With an invoice the date is billed: it stays.
    let service_id = common::seed_got_service(&pool).await;
    app.post(
        &format!("/api/treatments/{treatment_id}/items"),
        json!({ "kind": "service", "service_id": service_id, "quantity": "1" }),
    )
    .await;
    app.post(
        &format!("/api/treatments/{treatment_id}/invoice"),
        json!({}),
    )
    .await;
    let again = phone_event(
        &format!("kvet-{appointment_id}"),
        "DTSTART:20261201T090000Z",
        "egal",
    );
    let refused = app.dav("PUT", &href, &[], &again).await;
    assert_eq!(refused.status, StatusCode::NO_CONTENT);
    assert!(refused.header(header::ETAG).is_none());
    let kept: DateTime<Utc> = sqlx::query_scalar("SELECT starts_at FROM appointment WHERE id = $1")
        .bind(appointment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, starts_at);
}

/// Deleted on the phone: an appointment with nothing recorded goes, one with an animal on it
/// stays in k-vet and only leaves the calendar until shown again.
#[sqlx::test]
async fn a_deleted_event_goes_or_is_only_hidden(pool: PgPool) {
    let app = app(&pool).await;
    let empty = app
        .dav(
            "PUT",
            "/dav/calendars/praxis/leer.ics",
            &[],
            &phone_event("leer", "DTSTART:20261110T090000Z", "Rückruf"),
        )
        .await;
    assert_eq!(empty.status, StatusCode::CREATED);
    let gone = app
        .dav("DELETE", "/dav/calendars/praxis/leer.ics", &[], "")
        .await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM appointment WHERE calendar_href = 'leer.ics'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(left, 0);

    let (appointment_id, _) = visit(&pool, Utc::now() + TimeDelta::days(1)).await;
    let href = format!("/dav/calendars/praxis/kvet-{appointment_id}.ics");
    let hidden = app.dav("DELETE", &href, &[], "").await;
    assert_eq!(hidden.status, StatusCode::NO_CONTENT);
    assert!(etag_in(&listing(&app).await, &href).is_none());
    let appointment = app
        .get(&format!("/api/appointments/{appointment_id}"))
        .await
        .json();
    assert_eq!(appointment["calendar_hidden"], true);

    // "Wieder anzeigen".
    app.patch(
        &format!("/api/appointments/{appointment_id}"),
        json!({ "calendar_hidden": false }),
    )
    .await;
    assert!(etag_in(&listing(&app).await, &href).is_some());
}

/// An all-day or recurring entry cannot be an appointment: it is kept as it was sent and
/// served back unchanged, until deleted.
#[sqlx::test]
async fn all_day_entries_are_kept_as_they_are(pool: PgPool) {
    let app = app(&pool).await;
    let body = phone_event(
        "urlaub",
        "DTSTART;VALUE=DATE:20261224",
        "Praxis geschlossen",
    );
    let created = app
        .dav("PUT", "/dav/calendars/praxis/urlaub.ics", &[], &body)
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let appointments: i64 = sqlx::query_scalar("SELECT count(*) FROM appointment")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(appointments, 0);

    let served = app
        .dav("GET", "/dav/calendars/praxis/urlaub.ics", &[], "")
        .await;
    assert_eq!(text(&served), body);
    assert!(etag_in(&listing(&app).await, "/dav/calendars/praxis/urlaub.ics").is_some());

    let deleted = app
        .dav("DELETE", "/dav/calendars/praxis/urlaub.ics", &[], "")
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.dav("GET", "/dav/calendars/praxis/urlaub.ics", &[], "")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

/// `calendar-query` with a time range answers the events in it.
#[sqlx::test]
async fn a_calendar_query_keeps_to_its_time_range(pool: PgPool) {
    let app = app(&pool).await;
    let (soon, _) = visit(&pool, Utc::now() + TimeDelta::days(1)).await;
    let (later, _) = visit(&pool, Utc::now() + TimeDelta::days(40)).await;
    let until = (Utc::now() + TimeDelta::days(10)).format("%Y%m%dT%H%M%SZ");
    let query = format!(
        r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
           <d:prop><d:getetag/></d:prop><c:filter><c:comp-filter name="VCALENDAR">
           <c:comp-filter name="VEVENT"><c:time-range end="{until}"/></c:comp-filter>
           </c:comp-filter></c:filter></c:calendar-query>"#
    );
    let report = text(
        &app.dav(
            "REPORT",
            "/dav/calendars/praxis/",
            &[("Depth", "1")],
            &query,
        )
        .await,
    );
    assert!(etag_in(&report, &format!("/dav/calendars/praxis/kvet-{soon}.ics")).is_some());
    assert!(etag_in(&report, &format!("/dav/calendars/praxis/kvet-{later}.ics")).is_none());
}
