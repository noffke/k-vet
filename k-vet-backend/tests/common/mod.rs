//! Shared integration-test harness: a real router on a `#[sqlx::test]` database plus
//! the seed fixtures the story tests build on.

#![allow(dead_code)]
// Test helpers panic to report failures.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::cell::RefCell;
use std::sync::Arc;

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
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub attachments: TempDir,
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
        let mut settings = test_config(attachments.path().to_string_lossy().as_ref());
        adjust(&mut settings);
        let config = Arc::new(settings);
        let state = AppState::new(pool.clone(), Arc::clone(&config)).expect("state builds");
        let router = build_app(state).await.expect("app builds");
        Self {
            router,
            pool,
            config,
            attachments,
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
        [travel_expenses]
        rate_per_double_km = "3.50"
        minimum = "13.00"
        "#
    );
    Config::parse(&raw, "test.toml").expect("test config is valid")
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

/// A self-defined travel-expense service (price computed from kilometres).
pub async fn seed_travel_service(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO service (type, name, vat_percent, net_price, travel_expenses, draft)
         VALUES ('self_defined', 'Wegegeld', 19.000, 13.00, true, false) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed travel service")
}

pub async fn seed_appointment(pool: &PgPool, starts_at: DateTime<Utc>) -> i64 {
    sqlx::query_scalar("INSERT INTO appointment (starts_at, draft) VALUES ($1, false) RETURNING id")
        .bind(starts_at)
        .fetch_one(pool)
        .await
        .expect("seed appointment")
}

pub async fn seed_treatment(pool: &PgPool, appointment_id: i64, patient_id: i64) -> i64 {
    let treatment_id: i64 = sqlx::query_scalar(
        "INSERT INTO treatment (appointment_id, treatment_reason)
         VALUES ($1, 'Routinekontrolle') RETURNING id",
    )
    .bind(appointment_id)
    .fetch_one(pool)
    .await
    .expect("seed treatment");
    sqlx::query("INSERT INTO treatment_patient (treatment_id, patient_id) VALUES ($1, $2)")
        .bind(treatment_id)
        .bind(patient_id)
        .execute(pool)
        .await
        .expect("seed treatment patient");
    treatment_id
}

/// Remaining stock of a lot, straight from the `lot_remaining` view.
pub async fn lot_remaining(pool: &PgPool, lot_id: i64) -> Decimal {
    sqlx::query_scalar("SELECT remaining FROM lot_remaining WHERE lot_id = $1")
        .bind(lot_id)
        .fetch_one(pool)
        .await
        .expect("lot remaining")
}
