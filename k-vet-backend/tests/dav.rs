//! The appointments as a calendar and the customers as contacts on the phone (CalDAV and
//! CardDAV at `/dav`, migrations 0036 and 0037): the discovery a sync app walks, the events and
//! contacts it reads, and what it may write back.

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

// ── The customers as contacts (CardDAV) ─────────────────────────────────────────────────────

const PROPFIND_BOOK: &str = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"
    xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/"><d:prop>
    <d:resourcetype/><d:displayname/><card:addressbook-home-set/><card:supported-address-data/>
    <cs:getctag/><d:current-user-privilege-set/><d:getetag/></d:prop></d:propfind>"#;

fn contact_href(customer_id: i64) -> String {
    format!("/dav/addressbooks/kunden/kvet-customer-{customer_id}.vcf")
}

async fn book(app: &TestApp) -> String {
    let response = app
        .dav(
            "PROPFIND",
            "/dav/addressbooks/kunden/",
            &[("Depth", "1")],
            PROPFIND_BOOK,
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

async fn contact(app: &TestApp, href: &str) -> (String, String) {
    let response = app.dav("GET", href, &[], "").await;
    assert_eq!(response.status, StatusCode::OK, "{href}");
    assert_eq!(
        response.header(header::CONTENT_TYPE),
        Some("text/vcard; charset=utf-8")
    );
    let etag = response.header(header::ETAG).expect("an ETag").to_owned();
    (unfolded(&text(&response)), etag)
}

async fn customer_at(pool: &PgPool, href: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM customer WHERE dav_href = $1")
        .bind(href)
        .fetch_one(pool)
        .await
        .expect("a customer from the phone")
}

/// The same discovery, for the address book: the principal names its home, the home holds
/// "Kunden", which says it takes vCard 3.0 and may be written to.
#[sqlx::test]
async fn the_sync_app_finds_the_address_book(pool: PgPool) {
    let app = app(&pool).await;

    let well_known = app.dav("GET", "/.well-known/carddav", &[], "").await;
    assert!(well_known.status.is_redirection());
    assert_eq!(well_known.header(header::LOCATION), Some("/dav/"));

    let options = app.dav("OPTIONS", "/dav/", &[], "").await;
    assert!(
        options
            .header(HeaderName::from_static("dav"))
            .unwrap_or_default()
            .contains("addressbook")
    );

    let principal = app
        .dav(
            "PROPFIND",
            "/dav/principals/tierarzt/",
            &[("Depth", "0")],
            PROPFIND_BOOK,
        )
        .await;
    assert!(
        text(&principal).contains("<card:addressbook-home-set><d:href>/dav/addressbooks/</d:href>"),
        "{}",
        text(&principal)
    );

    let home = app
        .dav(
            "PROPFIND",
            "/dav/addressbooks/",
            &[("Depth", "1")],
            PROPFIND_BOOK,
        )
        .await;
    let home = text(&home);
    assert!(
        home.contains("<d:href>/dav/addressbooks/kunden/</d:href>"),
        "{home}"
    );
    assert!(home.contains("<d:collection/><card:addressbook/>"));
    assert!(home.contains("<d:displayname>Kunden</d:displayname>"));
    assert!(home.contains(r#"<card:address-data-type content-type="text/vcard" version="3.0"/>"#));
    assert!(home.contains("<d:privilege><d:write/></d:privilege>"));
    // The calendar is not in the address book's home.
    assert!(!home.contains("/dav/calendars/"));
}

/// Every customer with a name is a contact named with the living animals, carrying the numbers,
/// addresses and a note with the species — never the breed. A change to the customer, an animal
/// or an e-mail address is a new ETag and a new ctag; an archived customer drops out.
#[sqlx::test]
async fn customers_are_contacts(pool: PgPool) {
    let app = app(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    sqlx::query(
        "UPDATE customer SET phone = '+49301234567', mobile = '+491711234567' WHERE id = $1",
    )
    .bind(customer_id)
    .execute(&pool)
    .await
    .expect("numbers");
    let bello = common::seed_patient(&pool, customer_id).await;
    sqlx::query("UPDATE patient SET race = 'Labrador' WHERE id = $1")
        .bind(bello)
        .execute(&pool)
        .await
        .expect("a breed");
    for (name, gone) in [
        ("Minka", "date_of_death = '2025-01-01'"),
        ("Hansi", "archived = true"),
    ] {
        let id = common::seed_patient(&pool, customer_id).await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE patient SET name = $2, {gone} WHERE id = $1"
        )))
        .bind(id)
        .bind(name)
        .execute(&pool)
        .await
        .expect("an animal no longer there");
    }
    common::seed_customer_email(&pool, customer_id, "erika@example.com").await;
    // A draft nobody has named yet is not a contact.
    let unnamed = app.post("/api/customers", json!({})).await.id();

    let href = contact_href(customer_id);
    let listed = book(&app).await;
    let etag = etag_in(&listed, &href).expect("the customer is listed");
    assert!(!listed.contains(&contact_href(unnamed)));
    let ctag = ctag_in(&listed);

    let multiget = format!(
        r#"<card:addressbook-multiget xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
           <d:prop><d:getetag/><card:address-data/></d:prop><d:href>{href}</d:href>
           <d:href>/dav/addressbooks/kunden/unknown.vcf</d:href></card:addressbook-multiget>"#
    );
    let report = app
        .dav(
            "REPORT",
            "/dav/addressbooks/kunden/",
            &[("Depth", "1")],
            &multiget,
        )
        .await;
    assert_eq!(report.status, StatusCode::MULTI_STATUS);
    let report = text(&report);
    assert!(
        report.contains("<card:address-data>BEGIN:VCARD"),
        "{report}"
    );
    assert!(report.contains("404 Not Found"));
    assert_eq!(etag_in(&report, &href).as_deref(), Some(etag.as_str()));

    let (vcf, get_etag) = contact(&app, &href).await;
    assert_eq!(get_etag, etag);
    assert!(vcf.contains("FN:Erika Mustermann (Bello)\r\n"), "{vcf}");
    assert!(vcf.contains("TEL;TYPE=VOICE:+49301234567\r\n"));
    assert!(vcf.contains("TEL;TYPE=CELL:+491711234567\r\n"));
    assert!(vcf.contains("EMAIL;TYPE=INTERNET:erika@example.com\r\n"));
    assert!(vcf.contains("ADR;TYPE=HOME:;;Musterweg 5;Musterstadt;;12345;\r\n"));
    assert!(vcf.contains(&format!(
        "NOTE:Bello (Hund)\\nhttp://pi:8080/customers/{customer_id}\r\n"
    )));
    for absent in ["Labrador", "Minka", "Hansi"] {
        assert!(!vcf.contains(absent), "{absent} in {vcf}");
    }

    // An animal renamed, then an address added: each a new ETag and ctag.
    app.patch(&format!("/api/patients/{bello}"), json!({ "name": "Rex" }))
        .await;
    let renamed = book(&app).await;
    let renamed_etag = etag_in(&renamed, &href).expect("still listed");
    assert_ne!(renamed_etag, etag);
    assert_ne!(ctag_in(&renamed), ctag);
    assert!(
        contact(&app, &href)
            .await
            .0
            .contains("FN:Erika Mustermann (Rex)")
    );
    common::seed_customer_email(&pool, customer_id, "erika@work.example.com").await;
    assert_ne!(etag_in(&book(&app).await, &href), Some(renamed_etag));

    app.post_empty(&format!("/api/customers/{customer_id}/archive"))
        .await;
    assert_eq!(etag_in(&book(&app).await, &href), None);
    assert_eq!(
        app.dav("GET", &href, &[], "").await.status,
        StatusCode::NOT_FOUND
    );
}

/// A contact made on the phone becomes a customer — a draft, for want of a salutation — that
/// keeps the phone's name for it, and that the nightly cleanup leaves alone however little it
/// holds.
#[sqlx::test]
async fn a_contact_made_on_the_phone_becomes_a_customer(pool: PgPool) {
    let app = app(&pool).await;
    let href = "/dav/addressbooks/kunden/3f2a-heike.vcf";
    let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:3f2a-heike\r\nFN:Heike Müller\r\n\
                N:Müller;Heike;;;\r\nTEL;TYPE=CELL:0171 7654321\r\nTEL;TYPE=HOME:030 7654321\r\n\
                EMAIL;TYPE=INTERNET:heike@example.org\r\n\
                ADR;TYPE=HOME:;;Dorfstraße 1;Neustadt;;54321;\r\nEND:VCARD\r\n";
    let created = app.dav("PUT", href, &[("If-None-Match", "*")], card).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", text(&created));
    assert_eq!(
        created.header(header::ETAG),
        None,
        "k-vet's version is fetched back"
    );
    // The same name a second time is not a new contact.
    assert_eq!(
        app.dav("PUT", href, &[("If-None-Match", "*")], card)
            .await
            .status,
        StatusCode::PRECONDITION_FAILED
    );

    let id = customer_at(&pool, "3f2a-heike.vcf").await;
    let customer = app.get(&format!("/api/customers/{id}")).await.json();
    assert_eq!(customer["draft"], true);
    assert_eq!(customer["from_phone"], true);
    assert_eq!(customer["first_name"], "Heike");
    assert_eq!(customer["last_name"], "Müller");
    assert_eq!(customer["mobile"], "+491717654321");
    assert_eq!(customer["phone"], "+49307654321");
    assert_eq!(customer["home_street"], "Dorfstraße 1");
    assert_eq!(customer["emails"][0]["email"], "heike@example.org");

    let listed = app.get("/api/customers?q=Müller").await.json();
    assert!(
        listed
            .as_array()
            .expect("a list")
            .iter()
            .any(|row| row["id"] == id)
    );

    // Served back under the phone's own name and UID.
    let (vcf, _) = contact(&app, href).await;
    assert!(vcf.contains("UID:3f2a-heike\r\n"), "{vcf}");
    assert!(vcf.contains("FN:Heike Müller\r\n"));
    assert!(etag_in(&book(&app).await, href).is_some());

    // Only a company and a number: still kept by the cleanup, days later.
    let farm = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:farm\r\nFN:Gut Sonnenhof\r\nN:;;;;\r\n\
                ORG:Gut Sonnenhof\r\nTEL:0151 2223334\r\nEND:VCARD\r\n";
    assert_eq!(
        app.dav("PUT", "/dav/addressbooks/kunden/farm.vcf", &[], farm)
            .await
            .status,
        StatusCode::CREATED
    );
    let farm_id = customer_at(&pool, "farm.vcf").await;
    sqlx::query("UPDATE customer SET created_at = now() - interval '30 days' WHERE id = ANY($1)")
        .bind(vec![id, farm_id])
        .execute(&pool)
        .await
        .expect("aged");
    k_vet_backend::jobs::cleanup_abandoned_drafts(&pool)
        .await
        .expect("cleanup");
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM customer WHERE id = ANY($1)")
        .bind(vec![id, farm_id])
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(kept, 2);

    // Not a contact at all.
    assert_eq!(
        app.dav(
            "PUT",
            "/dav/addressbooks/kunden/x.vcf",
            &[],
            "BEGIN:VCALENDAR"
        )
        .await
        .status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

/// An edit on the phone changes the customer: the name from `N` — the "(Bello)" k-vet put on the
/// displayed name is not taken into it — the numbers, the e-mail addresses matched by address
/// (an existing one keeps its type) and the address. What makes a customer complete is changed,
/// never emptied; a number that does not read as one leaves the stored one.
#[sqlx::test]
async fn an_edit_on_the_phone_changes_the_customer(pool: PgPool) {
    let app = app(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    common::seed_patient(&pool, customer_id).await;
    let work = common::seed_customer_email(&pool, customer_id, "erika@example.com").await;
    sqlx::query("UPDATE customer_email SET email_type = 'work' WHERE id = $1")
        .bind(work)
        .execute(&pool)
        .await
        .expect("a work address");
    common::seed_customer_email(&pool, customer_id, "alt@example.com").await;
    let href = contact_href(customer_id);
    let (_, etag) = contact(&app, &href).await;

    let edited = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:kvet-customer-X\r\n\
                  FN:Erika Musterfrau (Bello)\r\nN:Musterfrau;Erika;;;\r\n\
                  TEL;TYPE=VOICE:+49301234567\r\nTEL;TYPE=CELL:0171 1112223\r\n\
                  EMAIL;TYPE=INTERNET:Erika@Example.com\r\nEMAIL;TYPE=INTERNET:neu@example.com\r\n\
                  NOTE:whatever the phone kept\r\nEND:VCARD\r\n";
    let updated = app
        .dav("PUT", &href, &[("If-Match", etag.as_str())], edited)
        .await;
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{}", text(&updated));
    assert_eq!(updated.header(header::ETAG), None);

    let customer = app
        .get(&format!("/api/customers/{customer_id}"))
        .await
        .json();
    assert_eq!(customer["last_name"], "Musterfrau");
    assert_eq!(customer["first_name"], "Erika");
    assert_eq!(customer["mobile"], "+491711112223");
    assert_eq!(customer["phone"], "+49301234567");
    // No address on the card: a complete customer keeps the one it has.
    assert_eq!(customer["home_street"], "Musterweg 5");
    assert_eq!(customer["draft"], false);
    assert_eq!(customer["from_phone"], false);
    let emails: Vec<(String, String)> = customer["emails"]
        .as_array()
        .expect("emails")
        .iter()
        .map(|email| {
            (
                email["email"].as_str().unwrap_or_default().to_owned(),
                email["email_type"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        emails,
        vec![
            ("erika@example.com".to_owned(), "work".to_owned()),
            ("neu@example.com".to_owned(), "private".to_owned()),
        ]
    );
    let (vcf, _) = contact(&app, &href).await;
    assert!(vcf.contains("FN:Erika Musterfrau (Bello)\r\n"), "{vcf}");
    assert!(!vcf.contains("whatever the phone kept"));
    // k-vet's UID stays, whatever the card says.
    assert!(vcf.contains(&format!("UID:kvet-customer-{customer_id}\r\n")));

    // The ETag the phone had is stale now.
    assert_eq!(
        app.dav("PUT", &href, &[("If-Match", etag.as_str())], edited)
            .await
            .status,
        StatusCode::PRECONDITION_FAILED
    );

    // A number that is none keeps the stored one; a removed one goes.
    let garbled = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Musterfrau;Erika;;;\r\n\
                   TEL;TYPE=VOICE:nicht erreichbar\r\nEND:VCARD\r\n";
    app.dav("PUT", &href, &[], garbled).await;
    let customer = app
        .get(&format!("/api/customers/{customer_id}"))
        .await
        .json();
    assert_eq!(customer["phone"], "+49301234567");
    assert_eq!(customer["mobile"], serde_json::Value::Null);
    assert_eq!(customer["emails"].as_array().map(Vec::len), Some(0));
}

/// A contact deleted on the phone is not a customer deleted: it stays, and is listed again.
#[sqlx::test]
async fn a_contact_deleted_on_the_phone_comes_back(pool: PgPool) {
    let app = app(&pool).await;
    let customer_id = common::seed_customer(&pool).await;
    let href = contact_href(customer_id);
    let (_, etag) = contact(&app, &href).await;

    assert_eq!(
        app.dav("DELETE", &href, &[("If-Match", "\"stale\"")], "")
            .await
            .status,
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        app.dav("DELETE", &href, &[("If-Match", etag.as_str())], "")
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&format!("/api/customers/{customer_id}"))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(etag_in(&book(&app).await, &href), Some(etag));
    assert_eq!(
        app.dav("DELETE", "/dav/addressbooks/kunden/unknown.vcf", &[], "")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
