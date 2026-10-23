//! The invoice mail queue (migration 0026): requests only queue, the job delivers, a failure is
//! kept on the row and retried for six hours — or not at all when the mail server refused.

// Integration tests may panic on unexpected results — that is how a test reports failure.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::{MailServer, TestApp};
use k_vet_backend::jobs::{self, MailRun};
use serde_json::{Value, json};
use sqlx::PgPool;

/// A treatment with one position and its invoice, written but not released yet.
async fn written_invoice(app: &TestApp, pool: &PgPool) -> i64 {
    let customer_id = common::seed_customer(pool).await;
    let patient_id = common::seed_patient(pool, customer_id).await;
    let appointment_id = common::seed_appointment(pool, Utc::now()).await;
    let treatment_id = common::seed_treatment(pool, appointment_id, patient_id).await;
    let service_id = common::seed_got_service(pool).await;
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
    invoice["id"].as_i64().expect("an invoice")
}

/// Such an invoice, released with `recipients` ticked and its mail due at `send_at`.
async fn released_at(
    app: &TestApp,
    pool: &PgPool,
    recipients: &[&str],
    send_at: chrono::DateTime<Utc>,
) -> i64 {
    let invoice_id = written_invoice(app, pool).await;
    let released = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": recipients, "send_at": send_at }),
        )
        .await;
    assert_eq!(released.status, StatusCode::OK);
    invoice_id
}

/// Released with "Gleich versenden".
async fn released_invoice(app: &TestApp, pool: &PgPool, recipients: &[&str]) -> i64 {
    released_at(app, pool, recipients, Utc::now()).await
}

async fn invoice(app: &TestApp, invoice_id: i64) -> Value {
    app.get(&format!("/api/invoices/{invoice_id}")).await.json()
}

async fn run(app: &TestApp) -> MailRun {
    jobs::send_queued_mail(&app.state)
        .await
        .expect("the queue runs")
}

async fn queued(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM mail_queue")
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Releasing only queues the mail; nothing reaches the mail server until the queue's run, which
/// delivers it, records the delivery on the invoice and empties the queue.
#[sqlx::test]
async fn releasing_queues_the_mail_and_the_run_delivers_it(pool: PgPool) {
    let mail = MailServer::start();
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let invoice_id = released_invoice(&app, &pool, &["kundin@example.com"]).await;

    let waiting = invoice(&app, invoice_id).await;
    assert_eq!(waiting["status"], "accepted");
    assert_eq!(
        waiting["email_queue"]["recipients"],
        json!(["kundin@example.com"])
    );
    assert!(
        mail.delivered().is_empty(),
        "the release itself sends nothing"
    );

    assert_eq!(
        run(&app).await,
        MailRun {
            sent: 1,
            ..MailRun::default()
        }
    );
    let sent = invoice(&app, invoice_id).await;
    assert_eq!(sent["status"], "sent");
    assert_eq!(sent["sent_email_to"], json!(["kundin@example.com"]));
    assert!(sent["email_queue"].is_null());
    assert_eq!(mail.delivered(), ["kundin@example.com"]);
    assert_eq!(queued(&pool).await, 0);

    assert_eq!(run(&app).await, MailRun::default(), "nothing is sent twice");
}

/// An unreachable mail server is tried again on every run, until six hours after the first
/// failure; then the queue gives up and leaves the mail alone.
#[sqlx::test]
async fn an_unreachable_mail_server_is_retried_for_six_hours(pool: PgPool) {
    // No mail server: the test configuration points at a port nothing listens on.
    let app = TestApp::new(pool.clone()).await;
    let invoice_id = released_invoice(&app, &pool, &["kundin@example.com"]).await;

    assert_eq!(run(&app).await.failed, 1);
    let failing = invoice(&app, invoice_id).await;
    let queue = &failing["email_queue"];
    assert_eq!(queue["attempts"], 1);
    assert_eq!(queue["refused"], false);
    assert!(queue["gave_up_at"].is_null());
    assert!(!queue["retry_until"].is_null());
    assert!(
        queue["last_error"]
            .as_str()
            .is_some_and(|error| error.to_lowercase().contains("connection")),
        "the transport's own words are kept: {}",
        queue["last_error"]
    );
    assert_eq!(run(&app).await.failed, 1, "tried again on the next run");

    // Six hours of failing later, the next failure is the last.
    sqlx::query("UPDATE mail_queue SET first_failure_at = now() - interval '6 hours 1 minute'")
        .execute(&pool)
        .await
        .expect("back-date the first failure");
    assert_eq!(run(&app).await.gave_up, 1);
    assert_eq!(
        run(&app).await,
        MailRun::default(),
        "no attempt after giving up"
    );

    let given_up = invoice(&app, invoice_id).await;
    assert_eq!(given_up["email_queue"]["attempts"], 3);
    assert!(!given_up["email_queue"]["gave_up_at"].is_null());
    assert_eq!(given_up["status"], "accepted", "still not at the customer");
}

/// A mail server that refuses a recipient for good is not asked again: the mail is given up at
/// once, with the server's answer — and nobody got it, the other recipients included.
#[sqlx::test]
async fn a_refused_recipient_is_given_up_at_once(pool: PgPool) {
    let mail = MailServer::start();
    mail.refuse_recipient("weg@example.com");
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let invoice_id =
        released_invoice(&app, &pool, &["kundin@example.com", "weg@example.com"]).await;

    assert_eq!(run(&app).await.gave_up, 1);
    let refused = invoice(&app, invoice_id).await;
    let queue = &refused["email_queue"];
    assert_eq!(queue["refused"], true);
    assert_eq!(queue["attempts"], 1);
    assert!(!queue["gave_up_at"].is_null());
    assert!(
        queue["last_error"]
            .as_str()
            .is_some_and(|error| error.contains("550")),
        "the mail server's answer: {}",
        queue["last_error"]
    );
    assert!(
        mail.delivered().is_empty(),
        "a refused message reaches nobody"
    );
    assert_eq!(run(&app).await, MailRun::default());
}

/// From the dashboard a failed mail is tried again from the start, or taken out of the queue —
/// which leaves the invoice released and unsent.
#[sqlx::test]
async fn a_failed_mail_can_be_retried_or_removed(pool: PgPool) {
    let mail = MailServer::start();
    mail.refuse();
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let retried = released_invoice(&app, &pool, &["kundin@example.com"]).await;
    let removed = released_invoice(&app, &pool, &["nachbar@example.com"]).await;
    assert_eq!(run(&app).await.failed, 2);

    let dashboard = app.get("/api/dashboard").await.json();
    let failed = dashboard["failed_mails"].as_array().expect("failed mails");
    assert_eq!(failed.len(), 2);
    assert!(failed[0]["mail"]["last_error"].is_string());
    assert!(
        !failed[0]["invoice_number"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );

    let retried_id = invoice(&app, retried).await["email_queue"]["id"].clone();
    let reset = app
        .post_empty(&format!("/api/mail-queue/{retried_id}/retry"))
        .await;
    assert_eq!(reset.status, StatusCode::OK);
    let reset = reset.json();
    assert!(reset["last_error"].is_null());
    assert_eq!(reset["attempts"], 0);

    let removed_id = invoice(&app, removed).await["email_queue"]["id"].clone();
    let gone = app.delete(&format!("/api/mail-queue/{removed_id}")).await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    let left_alone = invoice(&app, removed).await;
    assert!(left_alone["email_queue"].is_null());
    assert_eq!(left_alone["status"], "accepted");
    assert_eq!(
        app.delete(&format!("/api/mail-queue/{removed_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // The mail server is back: the retried mail goes out with the next run.
    mail.accepting
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(run(&app).await.sent, 1);
    assert_eq!(invoice(&app, retried).await["status"], "sent");
    assert_eq!(mail.delivered(), ["kundin@example.com"]);
    assert!(
        app.get("/api/dashboard").await.json()["failed_mails"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
}

/// One waiting mail per invoice: sending again replaces it, with a fresh start.
#[sqlx::test]
async fn sending_again_replaces_the_waiting_mail(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let invoice_id = released_invoice(&app, &pool, &["kundin@example.com"]).await;
    assert_eq!(run(&app).await.failed, 1);

    let resent = app
        .post(
            &format!("/api/invoices/{invoice_id}/send"),
            json!({ "recipient_emails": ["neu@example.com"], "send_at": Utc::now() }),
        )
        .await
        .json();
    assert_eq!(
        resent["email_queue"]["recipients"],
        json!(["neu@example.com"])
    );
    assert_eq!(resent["email_queue"]["attempts"], 0);
    assert!(resent["email_queue"]["last_error"].is_null());
    assert_eq!(queued(&pool).await, 1);
}

/// A cancelled invoice never reaches the customer, and one handed over on paper does not need
/// to arrive a second time: both drop the waiting mail.
#[sqlx::test]
async fn cancelling_or_posting_drops_the_waiting_mail(pool: PgPool) {
    let mail = MailServer::start();
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let posted = released_invoice(&app, &pool, &["kundin@example.com"]).await;
    let cancelled = released_invoice(&app, &pool, &["nachbar@example.com"]).await;

    let marked = app
        .post_empty(&format!("/api/invoices/{posted}/mark-posted"))
        .await;
    assert_eq!(marked.status, StatusCode::OK);
    assert!(marked.json()["email_queue"].is_null());
    let cancel = app
        .post_empty(&format!("/api/invoices/{cancelled}/cancel"))
        .await;
    assert_eq!(cancel.status, StatusCode::OK);
    assert_eq!(queued(&pool).await, 0);

    // A row that slipped past the cancel is dropped by the run, never sent.
    sqlx::query(
        "INSERT INTO mail_queue (invoice_id, recipients) VALUES ($1, '{nachbar@example.com}')",
    )
    .bind(cancelled)
    .execute(&pool)
    .await
    .expect("a stray queued mail");
    assert_eq!(
        run(&app).await,
        MailRun {
            dropped: 1,
            ..MailRun::default()
        }
    );
    assert!(mail.delivered().is_empty());
}

/// "Später versenden": the mail waits for its time — the invoice is released and done with,
/// the owners get it tomorrow morning — and goes out with the first run after it.
#[sqlx::test]
async fn a_mail_scheduled_for_later_waits_for_its_time(pool: PgPool) {
    let mail = MailServer::start();
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let tomorrow = Utc::now() + chrono::TimeDelta::hours(18);
    let invoice_id = released_at(&app, &pool, &["kundin@example.com"], tomorrow).await;

    let scheduled = invoice(&app, invoice_id).await;
    let send_after = scheduled["email_queue"]["send_after"]
        .as_str()
        .and_then(|value| value.parse::<chrono::DateTime<Utc>>().ok())
        .expect("a schedule");
    assert_eq!(send_after.timestamp(), tomorrow.timestamp());
    assert_eq!(run(&app).await, MailRun::default(), "not before its time");
    assert!(mail.delivered().is_empty());

    // The dashboard tells it apart from an invoice nobody sent.
    let dashboard = app.get("/api/dashboard").await.json();
    let entry = dashboard["unsent"]["entries"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["invoice_number"] == scheduled["invoice_number"])
        })
        .expect("still not at the customer");
    assert!(!entry["mail_scheduled_for"].is_null());

    sqlx::query("UPDATE mail_queue SET send_after = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .expect("the time has come");
    assert_eq!(run(&app).await.sent, 1);
    assert_eq!(mail.delivered(), ["kundin@example.com"]);
}

/// "Gleich versenden" is now: sending again with now turns a scheduled mail into one for the
/// next run — and a time already past (a browser clock a little behind) means the same.
#[sqlx::test]
async fn now_or_a_past_time_goes_out_with_the_next_run(pool: PgPool) {
    let mail = MailServer::start();
    let app = TestApp::with_config(pool.clone(), |config| config.mail.smtp_port = mail.port).await;
    let rescheduled = released_at(
        &app,
        &pool,
        &["kundin@example.com"],
        Utc::now() + chrono::TimeDelta::days(1),
    )
    .await;
    let resent = app
        .post(
            &format!("/api/invoices/{rescheduled}/send"),
            json!({ "recipient_emails": ["kundin@example.com"], "send_at": Utc::now() }),
        )
        .await;
    assert_eq!(resent.status, StatusCode::OK);
    released_at(
        &app,
        &pool,
        &["nachbar@example.com"],
        Utc::now() - chrono::TimeDelta::minutes(2),
    )
    .await;

    assert_eq!(run(&app).await.sent, 2);
    assert_eq!(
        mail.delivered(),
        ["kundin@example.com", "nachbar@example.com"]
    );
}

/// When the mail goes out is always said: a release or send without `send_at` is refused.
#[sqlx::test]
async fn a_release_must_say_when_its_mail_goes_out(pool: PgPool) {
    let app = TestApp::new(pool.clone()).await;
    let invoice_id = written_invoice(&app, &pool).await;
    let refused = app
        .post(
            &format!("/api/invoices/{invoice_id}/accept"),
            json!({ "recipient_emails": ["kundin@example.com"] }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(queued(&pool).await, 0);
}
