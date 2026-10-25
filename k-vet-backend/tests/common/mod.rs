//! Shared integration-test harness: a real router on a `#[sqlx::test]` database plus
//! the seed fixtures the story tests build on.

#![allow(dead_code)]
// Test helpers panic to report failures.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, NaiveDate, Utc};
use http_body_util::BodyExt;
use k_vet_backend::config::Config;
use k_vet_backend::{AppState, build_app};
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use tempfile::TempDir;
use tower::ServiceExt;

pub const TEST_USER: &str = "tierarzt";
pub const TEST_PASSWORD: &str = "test1234";
const TEST_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$sv7lVNC/98GKqHKTWF2RnA$e/w5YttGxthEXoRvReBLS4+Y1NQOvwaFcRGrtg6JXNU";

/// An in-process application under test, with a cookie jar for the session.
pub struct TestApp {
    router: Router,
    /// What the router runs on — for the jobs, which tests call directly.
    pub state: AppState,
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub attachments: TempDir,
    /// The templates folder: the seeded `invoice-email.txt` and `invoice-email-kurz.txt`; the
    /// third configured template, "Fehlt", has no file.
    pub templates: TempDir,
    /// Session cookie jar — interior mutability keeps every request method `&self`.
    cookie: RefCell<Option<String>>,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub headers: axum::http::HeaderMap,
}

impl TestResponse {
    /// A response header as a string, for cache/etag assertions.
    pub fn header(&self, name: header::HeaderName) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "response body is not JSON ({error}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn parse<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "response body does not match the expected shape ({error}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn id(&self) -> i64 {
        self.json()["id"].as_i64().expect("response carries an id")
    }

    /// Field names reported in an RFC 7807 validation problem.
    pub fn error_fields(&self) -> Vec<String> {
        self.json()["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .filter_map(|error| error["field"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl TestApp {
    /// Builds the app on `pool` and logs in, so tests start authenticated.
    pub async fn new(pool: PgPool) -> Self {
        let app = Self::anonymous(pool).await;
        let response = app.login(TEST_USER, TEST_PASSWORD).await;
        assert_eq!(response.status, StatusCode::OK, "test login must succeed");
        app
    }

    /// Builds the app with the configuration tweaked, and logs in.
    ///
    /// For the handful of settings whose whole point is that they change behaviour — the
    /// AMPreisV Teilmengen floor, for one — a test has to be able to run both ways.
    pub async fn with_config(pool: PgPool, adjust: impl FnOnce(&mut Config)) -> Self {
        let app = Self::anonymous_with_config(pool, adjust).await;
        let response = app.login(TEST_USER, TEST_PASSWORD).await;
        assert_eq!(response.status, StatusCode::OK, "test login must succeed");
        app
    }

    /// Builds the app without logging in.
    pub async fn anonymous(pool: PgPool) -> Self {
        Self::anonymous_with_config(pool, |_| {}).await
    }

    async fn anonymous_with_config(pool: PgPool, adjust: impl FnOnce(&mut Config)) -> Self {
        let attachments = TempDir::new().expect("temp dir for attachments");
        let templates = templates_folder();
        let mut settings = test_config(attachments.path().to_string_lossy().as_ref());
        settings.invoice.templates_dir = templates.path().to_path_buf();
        adjust(&mut settings);
        let config = Arc::new(settings);
        let state = AppState::new(pool.clone(), Arc::clone(&config)).expect("state builds");
        let router = build_app(state.clone()).await.expect("app builds");
        Self {
            router,
            state,
            pool,
            config,
            attachments,
            templates,
            cookie: RefCell::new(None),
        }
    }

    pub async fn login(&self, username: &str, password: &str) -> TestResponse {
        self.send(
            Method::POST,
            "/api/auth/login",
            Some(serde_json::json!({ "username": username, "password": password })),
        )
        .await
    }

    /// Drops the client-side session cookie without touching the server-side session.
    pub fn forget_session(&self) {
        *self.cookie.borrow_mut() = None;
    }

    /// The current session cookie, e.g. to replay it against a restarted app.
    pub fn session_cookie(&self) -> Option<String> {
        self.cookie.borrow().clone()
    }

    pub fn set_session_cookie(&self, cookie: &str) {
        *self.cookie.borrow_mut() = Some(cookie.to_owned());
    }

    pub async fn get(&self, uri: &str) -> TestResponse {
        self.send(Method::GET, uri, None).await
    }

    pub async fn post(&self, uri: &str, body: Value) -> TestResponse {
        self.send(Method::POST, uri, Some(body)).await
    }

    pub async fn post_empty(&self, uri: &str) -> TestResponse {
        self.send(Method::POST, uri, Some(serde_json::json!({})))
            .await
    }

    pub async fn patch(&self, uri: &str, body: Value) -> TestResponse {
        self.send(Method::PATCH, uri, Some(body)).await
    }

    pub async fn delete(&self, uri: &str) -> TestResponse {
        self.send(Method::DELETE, uri, None).await
    }

    /// Posts a `multipart/form-data` upload: one file part plus text parts.
    pub async fn post_multipart(
        &self,
        uri: &str,
        file_name: &str,
        content_type: &str,
        content: &[u8],
        text_parts: &[(&str, &str)],
    ) -> TestResponse {
        const BOUNDARY: &str = "kvetboundary42";
        let mut body: Vec<u8> = Vec::new();
        for (name, value) in text_parts {
            body.extend_from_slice(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        self.send_raw(
            Method::POST,
            uri,
            &format!("multipart/form-data; boundary={BOUNDARY}"),
            body,
        )
        .await
    }

    /// Sends a request, remembering any session cookie the server sets.
    pub async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> TestResponse {
        match body {
            Some(json) => {
                let bytes = serde_json::to_vec(&json).expect("body serializes");
                self.send_raw(method, uri, "application/json", bytes).await
            }
            None => self.dispatch(method, uri, None).await,
        }
    }

    /// Sends a request with an explicit content type and raw body.
    pub async fn send_raw(
        &self,
        method: Method,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> TestResponse {
        self.dispatch(method, uri, Some((content_type.to_owned(), body)))
            .await
    }

    async fn dispatch(
        &self,
        method: Method,
        uri: &str,
        body: Option<(String, Vec<u8>)>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(cookie) = self.cookie.borrow().as_ref() {
            builder = builder.header(header::COOKIE, cookie);
        }
        let request = match body {
            Some((content_type, bytes)) => builder
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(bytes)),
            None => builder.body(Body::empty()),
        }
        .expect("request builds");

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router answers");

        let status = response.status();
        let headers = response.headers().clone();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::to_owned);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body collects")
            .to_bytes()
            .to_vec();

        if let Some(cookie) = set_cookie {
            *self.cookie.borrow_mut() = Some(cookie);
        }
        TestResponse {
            status,
            body,
            content_type,
            headers,
        }
    }
}

/// A test configuration wired to a temporary attachments directory.
/// A mail server for the tests, which have none otherwise — without it only the failing side of
/// sending is ever exercised.
pub struct MailServer {
    pub port: u16,
    /// The envelope recipients (`RCPT TO`) of every message the server took: where mail
    /// really went, BCC included. A message refused part-way delivers to nobody, so its
    /// recipients are only counted once its `DATA` is in.
    pub recipients: Arc<Mutex<Vec<String>>>,
    /// Cleared, the server turns every connection away, as an unreachable relay would.
    pub accepting: Arc<AtomicBool>,
    /// Addresses the server does not know: `550` on their `RCPT TO`.
    pub unknown: Arc<Mutex<Vec<String>>>,
    /// The text of every message the server took, headers included.
    pub messages: Arc<Mutex<Vec<String>>>,
}

impl MailServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port for the mail server");
        let port = listener
            .local_addr()
            .expect("the mail server's address")
            .port();
        let recipients = Arc::new(Mutex::new(Vec::new()));
        let accepting = Arc::new(AtomicBool::new(true));
        let unknown = Arc::new(Mutex::new(Vec::new()));
        let messages = Arc::new(Mutex::new(Vec::new()));
        let mailbox = Mailbox {
            seen: Arc::clone(&recipients),
            accepting: Arc::clone(&accepting),
            unknown: Arc::clone(&unknown),
            messages: Arc::clone(&messages),
        };
        std::thread::spawn(move || {
            for connection in listener.incoming().flatten() {
                let mailbox = mailbox.clone();
                std::thread::spawn(move || serve_smtp(connection, &mailbox));
            }
        });
        MailServer {
            port,
            recipients,
            accepting,
            unknown,
            messages,
        }
    }

    pub fn refuse(&self) {
        self.accepting.store(false, Ordering::SeqCst);
    }

    /// The server answers `550` for `address`, as one would for a mailbox it does not have.
    pub fn refuse_recipient(&self, address: &str) {
        if let Ok(mut unknown) = self.unknown.lock() {
            unknown.push(address.to_owned());
        }
    }

    /// Every envelope recipient of the messages taken so far, sorted.
    pub fn delivered(&self) -> Vec<String> {
        let mut delivered = self.recipients.lock().expect("recipients").clone();
        delivered.sort();
        delivered
    }
}

/// What a connection of the test mail server shares with the test.
#[derive(Clone)]
struct Mailbox {
    seen: Arc<Mutex<Vec<String>>>,
    accepting: Arc<AtomicBool>,
    unknown: Arc<Mutex<Vec<String>>>,
    messages: Arc<Mutex<Vec<String>>>,
}

/// Just enough SMTP for lettre without TLS or authentication: every command is fine but a
/// recipient it was told not to know, `DATA` takes everything up to the lone dot.
fn serve_smtp(connection: TcpStream, mailbox: &Mailbox) {
    let Mailbox {
        seen,
        accepting,
        unknown,
        messages,
    } = mailbox;
    let Ok(reader) = connection.try_clone() else {
        return;
    };
    let mut writer = connection;
    let mut reply = |line: &str| writer.write_all(format!("{line}\r\n").as_bytes()).is_ok();
    if !accepting.load(Ordering::SeqCst) {
        reply("421 not now");
        return;
    }
    if !reply("220 k-vet test mail server") {
        return;
    }
    let mut in_data = false;
    let mut envelope = Vec::new();
    let mut text = String::new();
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else {
            return;
        };
        if in_data {
            if line == "." {
                in_data = false;
                if let Ok(mut seen) = seen.lock() {
                    seen.append(&mut envelope);
                }
                if let Ok(mut messages) = messages.lock() {
                    messages.push(std::mem::take(&mut text));
                }
                if !reply("250 queued") {
                    return;
                }
            } else {
                text.push_str(&line);
                text.push('\n');
            }
            continue;
        }
        // Pooled connections stay open between messages, so the switch is read per command.
        if !accepting.load(Ordering::SeqCst) {
            reply("421 not now");
            return;
        }
        let command = line.to_ascii_uppercase();
        let answer = if let Some(address) = command.strip_prefix("RCPT TO:") {
            let address = line[line.len() - address.len()..]
                .trim()
                .trim_matches(['<', '>'])
                .to_owned();
            let stranger = unknown
                .lock()
                .map(|unknown| unknown.contains(&address))
                .unwrap_or(false);
            if stranger {
                "550 no such user here"
            } else {
                envelope.push(address);
                "250 ok"
            }
        } else if command.starts_with("MAIL FROM:") || command.starts_with("RSET") {
            envelope.clear();
            "250 ok"
        } else if command.starts_with("DATA") {
            in_data = true;
            "354 go ahead"
        } else if command.starts_with("QUIT") {
            reply("221 bye");
            return;
        } else {
            "250 ok"
        };
        if !reply(answer) {
            return;
        }
    }
}

pub fn test_config(attachments_dir: &str) -> Config {
    let raw = format!(
        r#"
        [server]
        base_url = "http://127.0.0.1:8080"
        [auth]
        username = "{TEST_USER}"
        password_hash = "{TEST_PASSWORD_HASH}"
        [database]
        url = "postgres://unused/unused"
        [storage]
        attachments_dir = "{attachments_dir}"
        [mail]
        smtp_host = "127.0.0.1"
        smtp_port = 2525
        smtp_tls = "none"
        from_address = "praxis@example.com"
        from_name = "Tierarztpraxis Test"
        [invoice]
        number_pattern = "{{year}}-{{counter:4}}"
        [[invoice.email_templates]]
        name = "Standard"
        file = "invoice-email.txt"
        [[invoice.email_templates]]
        name = "Kurz"
        file = "invoice-email-kurz.txt"
        [[invoice.email_templates]]
        name = "Fehlt"
        file = "invoice-email-fehlt.txt"
        [travel_expenses]
        rate_per_double_km = "3.50"
        minimum = "13.00"
        "#
    );
    Config::parse(&raw, "test.toml").expect("test config is valid")
}

/// The subject line of the "Kurz" template, so a test can tell which template a mail used.
pub const SHORT_TEMPLATE_SUBJECT: &str = "Kurz: Rechnung";

/// A templates folder as the container seeds it, plus a second template of the test's own.
fn templates_folder() -> TempDir {
    let folder = TempDir::new().expect("temp dir for templates");
    std::fs::write(
        folder.path().join("invoice-email.txt"),
        include_str!("../../templates/invoice-email.txt"),
    )
    .expect("the seeded template");
    std::fs::write(
        folder.path().join("invoice-email-kurz.txt"),
        format!("{SHORT_TEMPLATE_SUBJECT} {{{{ invoice_number }}}}\n\n{{{{ greeting }}}},\nanbei die Rechnung.\n"),
    )
    .expect("a second template");
    folder
}

// ---------------------------------------------------------------------------
// Seed fixtures (T023): direct SQL so story tests do not depend on other stories' APIs.
// ---------------------------------------------------------------------------

pub async fn seed_customer(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO customer
             (salutation, first_name, last_name, home_street, home_zip, home_city, draft)
         VALUES ('frau', 'Erika', 'Mustermann', 'Musterweg 5', '12345', 'Musterstadt', false)
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed customer")
}

pub async fn seed_customer_email(pool: &PgPool, customer_id: i64, email: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO customer_email (customer_id, email, email_type)
         VALUES ($1, $2, 'private') RETURNING id",
    )
    .bind(customer_id)
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed customer email")
}

pub async fn seed_patient(pool: &PgPool, customer_id: i64) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO patient (customer_id, name, sex, species, draft)
         VALUES ($1, 'Bello', 'male', 'Hund', false) RETURNING id",
    )
    .bind(customer_id)
    .fetch_one(pool)
    .await
    .expect("seed patient")
}

pub async fn seed_manufacturer(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO manufacturer (name, draft) VALUES ('Pharma AG', false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed manufacturer")
}

pub async fn seed_supplier(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO supplier (name, draft) VALUES ('Großhandel GmbH', false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed supplier")
}

pub struct SeededDrug {
    pub drug_id: i64,
    /// Original packaging: one 100 ml bottle at 12.50 EUR gross.
    pub packaging_id: i64,
    /// Subset packaging: 10 ml dispensed from the bottle, 2.50 EUR gross.
    pub subset_packaging_id: i64,
}

/// A complete drug with an original packaging (100 ml bottle) and a 10 ml subset, 19% VAT.
///
/// Stock lives on the original packaging; billing a subset takes its `quantity` in base
/// units off the shelf.
pub async fn seed_drug(pool: &PgPool) -> SeededDrug {
    let manufacturer_id = seed_manufacturer(pool).await;
    let supplier_id = seed_supplier(pool).await;
    let drug_id: i64 = sqlx::query_scalar(
        "INSERT INTO drug (name, manufacturer_id, vat_percent, draft)
         VALUES ('Amoxicillin 100', $1, 19.000, false) RETURNING id",
    )
    .bind(manufacturer_id)
    .fetch_one(pool)
    .await
    .expect("seed drug");
    let packaging_id: i64 = sqlx::query_scalar(
        "INSERT INTO drug_packaging
             (drug_id, kind, unit, quantity, list_price_net, sales_price_net, supplier_id, draft)
         VALUES ($1, 'original', 'ml', 100.00, 10.00, 12.50, $2, false) RETURNING id",
    )
    .bind(drug_id)
    .bind(supplier_id)
    .fetch_one(pool)
    .await
    .expect("seed packaging");
    let subset_packaging_id: i64 = sqlx::query_scalar(
        "INSERT INTO drug_packaging
             (drug_id, kind, unit, quantity, list_price_net, sales_price_net, draft)
         VALUES ($1, 'subset', 'ml', 10.00, 1.50, 2.50, false) RETURNING id",
    )
    .bind(drug_id)
    .fetch_one(pool)
    .await
    .expect("seed subset packaging");

    SeededDrug {
        drug_id,
        packaging_id,
        subset_packaging_id,
    }
}

/// A stock lot for `packaging_id`. `expiration` steers FEFO ordering.
pub async fn seed_lot(
    pool: &PgPool,
    packaging_id: i64,
    packages: i32,
    quantity_per_package: Decimal,
    expiration: Option<NaiveDate>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO drug_stock_lot
             (packaging_id, packaging_kind, packages_received, initial_quantity,
              batch_number, expiration_date)
         VALUES ($1, 'original', $2, $3, $4, $5) RETURNING id",
    )
    .bind(packaging_id)
    .bind(packages)
    .bind(quantity_per_package * Decimal::from(packages))
    .bind(format!("BATCH-{packaging_id}-{packages}"))
    .bind(expiration)
    .fetch_one(pool)
    .await
    .expect("seed lot")
}

/// A GOT service: general examination, 100% factor, 19% VAT, 23.62 EUR gross.
pub async fn seed_got_service(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO service (type, name, got_number, factor, vat_percent, net_price, draft)
         VALUES ('got', 'Allgemeine Untersuchung', '1', 100.000, 19.000, 23.62, false)
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed GOT service")
}

/// A self-defined travel-expense service (price computed from kilometres) — per visit, as
/// every travel-expense service is (migration 0024).
pub async fn seed_travel_service(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO service
             (type, name, vat_percent, net_price, travel_expenses, per_visit, draft)
         VALUES ('self_defined', 'Wegegeld', 19.000, 13.00, true, true, false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed travel service")
}

/// A self-defined service marked "pro Termin": its lines belong to the visit, not an animal.
pub async fn seed_visit_service(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, per_visit, draft)
         VALUES ('self_defined', 'Hausbesuch', 19.000, 34.50, true, false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed per-visit service")
}

/// A complete appointment with its (still empty) treatment — an appointment never exists
/// without one (migration 0022), so neither does a seeded one.
pub async fn seed_appointment(pool: &PgPool, starts_at: DateTime<Utc>) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO appointment (starts_at, draft) VALUES ($1, false) RETURNING id",
    )
    .bind(starts_at)
    .fetch_one(pool)
    .await
    .expect("seed appointment");
    sqlx::query("INSERT INTO treatment (appointment_id) VALUES ($1)")
        .bind(id)
        .execute(pool)
        .await
        .expect("seed the appointment's treatment");
    id
}

/// A formatted text as the editor writes it, one plain paragraph per line — what a test means
/// by "the reason is …" now that the reason is a document (migration `0030`).
pub fn doc(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .lines()
        .map(|line| {
            if line.is_empty() {
                serde_json::json!({ "type": "paragraph" })
            } else {
                serde_json::json!({
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": line }],
                })
            }
        })
        .collect();
    serde_json::json!({ "type": "doc", "content": paragraphs })
}

pub async fn seed_treatment(pool: &PgPool, appointment_id: i64, patient_id: i64) -> i64 {
    seed_patient_treatment(pool, appointment_id, patient_id)
        .await
        .0
}

/// Puts an animal on the appointment's treatment, returning the treatment's and the record's
/// ids: the reason and the finding live on the animal's record, and a line's owner is that
/// record. The appointment takes the animal's customer if it has none yet.
pub async fn seed_patient_treatment(
    pool: &PgPool,
    appointment_id: i64,
    patient_id: i64,
) -> (i64, i64) {
    // The customer travels appointment -> treatment -> record, and the composite keys hold it
    // to one all the way down, so the seed has to set it at every level rather than leave the
    // NULLs a foreign key would simply skip over.
    let customer_id: i64 = sqlx::query_scalar("SELECT customer_id FROM patient WHERE id = $1")
        .bind(patient_id)
        .fetch_one(pool)
        .await
        .expect("the seeded animal has an owner");

    sqlx::query("UPDATE appointment SET customer_id = $2 WHERE id = $1 AND customer_id IS NULL")
        .bind(appointment_id)
        .bind(customer_id)
        .execute(pool)
        .await
        .expect("give the appointment its customer");

    // The appointment's one treatment, made with it; a first customer is carried down by hand,
    // as the API does — no key change for a cascade to see.
    let treatment_id: i64 = sqlx::query_scalar(
        "UPDATE treatment SET customer_id = COALESCE(customer_id, $2)
         WHERE appointment_id = $1 RETURNING id",
    )
    .bind(appointment_id)
    .bind(customer_id)
    .fetch_one(pool)
    .await
    .expect("seed treatment");
    // Some tests seed a database migrated only part of the way, to test one migration on the
    // rows before it; until `0030` the reason was plain text.
    let formatted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'rich_text_from_plain')",
    )
    .fetch_one(pool)
    .await
    .expect("look for migration 0030");
    let insert = if formatted {
        "INSERT INTO patient_treatment (treatment_id, patient_id, customer_id, treatment_reason)
         VALUES ($1, $2, $3, rich_text_from_plain('Routinekontrolle')) RETURNING id"
    } else {
        "INSERT INTO patient_treatment (treatment_id, patient_id, customer_id, treatment_reason)
         VALUES ($1, $2, $3, 'Routinekontrolle') RETURNING id"
    };
    let record_id: i64 = sqlx::query_scalar(insert)
        .bind(treatment_id)
        .bind(patient_id)
        .bind(customer_id)
        .fetch_one(pool)
        .await
        .expect("seed patient treatment");
    (treatment_id, record_id)
}

/// Remaining stock of a lot, straight from the `lot_remaining` view.
pub async fn lot_remaining(pool: &PgPool, lot_id: i64) -> Decimal {
    sqlx::query_scalar("SELECT remaining FROM lot_remaining WHERE lot_id = $1")
        .bind(lot_id)
        .fetch_one(pool)
        .await
        .expect("lot remaining")
}

// ---------------------------------------------------------------------------
// Google Maps
// ---------------------------------------------------------------------------

/// A stand-in for Google's Routes and Address Validation APIs, so the Maps code runs without a
/// key or a network: it records every request and gives the answer the test set.
pub struct FakeGoogle {
    pub url: String,
    state: Arc<Mutex<FakeGoogleState>>,
}

struct FakeGoogleState {
    requests: Vec<GoogleRequest>,
    routes: (StatusCode, Value),
    address: (StatusCode, Value),
}

/// One request the fake took.
#[derive(Debug, Clone)]
pub struct GoogleRequest {
    pub path: String,
    pub api_key: Option<String>,
    pub field_mask: Option<String>,
    pub body: Value,
}

impl FakeGoogle {
    /// Answers routes with two of them — the default 12.4 km, the shorter 11.9 km — to a house,
    /// and addresses as found.
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port for the fake Google");
        let port = listener.local_addr().expect("the fake's address").port();
        let state = Arc::new(Mutex::new(FakeGoogleState {
            requests: Vec::new(),
            routes: (StatusCode::OK, two_routes(12_437, 11_902)),
            address: (
                StatusCode::OK,
                address_answer("PREMISE", "CONFIRMED", false, "Musterweg 5"),
            ),
        }));
        let shared = Arc::clone(&state);
        let app = Router::new().fallback(move |request: Request<Body>| {
            let shared = Arc::clone(&shared);
            async move { fake_google(shared, request).await }
        });
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            state,
        }
    }

    /// Points a test configuration at this fake, with a key.
    pub fn configure(&self, config: &mut Config) {
        config.maps.api_key = "test-key".to_owned();
        config.maps.endpoint_override = Some(self.url.clone());
    }

    pub fn answer_routes(&self, status: StatusCode, body: Value) {
        self.state.lock().expect("fake state").routes = (status, body);
    }

    pub fn answer_address(&self, status: StatusCode, body: Value) {
        self.state.lock().expect("fake state").address = (status, body);
    }

    pub fn requests(&self) -> Vec<GoogleRequest> {
        self.state.lock().expect("fake state").requests.clone()
    }
}

/// A Routes API answer: the default route and the shorter one, to a house.
pub fn two_routes(default_meters: u64, shorter_meters: u64) -> Value {
    serde_json::json!({
        "routes": [
            { "distanceMeters": default_meters, "routeLabels": ["DEFAULT_ROUTE"] },
            { "distanceMeters": shorter_meters, "routeLabels": ["SHORTER_DISTANCE"] },
        ],
        "geocodingResults": {
            "origin": { "type": ["street_address"] },
            "destination": { "type": ["street_address"] },
        },
    })
}

/// An Address Validation answer for `line` in 12345 Musterstadt.
pub fn address_answer(granularity: &str, confirmation: &str, corrected: bool, line: &str) -> Value {
    serde_json::json!({
        "result": {
            "verdict": { "validationGranularity": granularity, "addressComplete": true },
            "address": {
                "formattedAddress": format!("{line}, 12345 Musterstadt, Deutschland"),
                "postalAddress": {
                    "regionCode": "DE",
                    "postalCode": "12345",
                    "locality": "Musterstadt",
                    "addressLines": [line],
                },
                "addressComponents": [
                    {
                        "componentName": { "text": line },
                        "componentType": "route",
                        "confirmationLevel": confirmation,
                        "spellCorrected": corrected,
                    },
                ],
            },
        },
    })
}

async fn fake_google(
    state: Arc<Mutex<FakeGoogleState>>,
    request: Request<Body>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let path = request.uri().path().to_owned();
    // Read before the body is awaited: a borrow of the request would make the future !Send.
    let (api_key, field_mask) = {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        (header("x-goog-api-key"), header("x-goog-fieldmask"))
    };
    let bytes = request
        .into_body()
        .collect()
        .await
        .expect("the request body")
        .to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    let mut state = state.lock().expect("fake state");
    state.requests.push(GoogleRequest {
        path: path.clone(),
        api_key,
        field_mask,
        body,
    });
    let (status, answer) = if path.ends_with(":computeRoutes") {
        state.routes.clone()
    } else {
        state.address.clone()
    };
    (status, axum::Json(answer)).into_response()
}

/// Gives the practice an address to drive from. Done before any customer is seeded: moving the
/// practice marks every existing customer's kilometres as outdated (migration 0031).
pub async fn seed_practice_address(pool: &PgPool) {
    sqlx::query(
        "UPDATE global_settings
         SET practice_street = 'Praxisweg 1', practice_zip = '12345', practice_city = 'Praxisstadt'
         WHERE id",
    )
    .execute(pool)
    .await
    .expect("seed the practice address");
}
