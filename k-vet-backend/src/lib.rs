//! k-vet backend — single-user veterinary practice management.
//!
//! The binary in `main.rs` is a thin wrapper; everything lives here so integration tests
//! can drive the real router in-process against a `#[sqlx::test]` database.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Router, middleware};
use sqlx::PgPool;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::cookie::time::Duration as CookieDuration;
use tower_sessions::{Expiry, SessionManagerLayer};

pub mod api;
pub mod auth;
pub mod config;
pub mod domain;
pub mod error;
pub mod jobs;
pub mod mail;
pub mod metrics;
pub mod pdf;
pub mod session_store;
pub mod static_assets;

use crate::auth::LoginThrottle;
use crate::config::Config;
use crate::domain::files::AttachmentStore;
use crate::error::{AppError, AppResult};
use crate::mail::Mailer;

/// Sessions survive a restart of the binary and stay valid for two weeks of use.
const SESSION_LIFETIME_DAYS: i64 = 14;

/// The OpenAPI document — single source of truth for the generated TypeScript client.
///
/// `cargo run --bin export-openapi > openapi.json` writes it; CI regenerates and diffs.
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "k-vet API",
        description = "Veterinary practice management — single user, self-hosted.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(
        auth::login,
        auth::logout,
        auth::session_info,
        api::attachments::upload,
        api::attachments::download,
        api::attachments::thumbnail,
        api::appointments::list,
        api::appointments::create,
        api::appointments::detail,
        api::appointments::patch,
        api::appointments::delete,
        api::appointments::duplicate,
        api::treatments::list_for_appointment,
        api::treatments::create_for_appointment,
        api::treatments::detail,
        api::treatments::patch,
        api::treatments::delete,
        api::treatments::add_patient,
        api::treatments::remove_patient,
        api::treatments::apply_template,
        api::treatments::duplicate,
        api::treatment_items::list,
        api::treatment_items::create,
        api::treatment_items::patch,
        api::treatment_items::delete,
        api::treatment_items::move_item,
        api::treatment_items::set_lots,
        api::customers::list,
        api::customers::create,
        api::customers::detail,
        api::customers::patch,
        api::customers::archive,
        api::customers::unarchive,
        api::customers::add_email,
        api::customers::patch_email,
        api::customers::delete_email,
        api::patients::list,
        api::patients::create,
        api::patients::detail,
        api::patients::patch,
        api::patients::archive,
        api::patients::unarchive,
        api::patients::list_files,
        api::patients::patch_file,
        api::suppliers::list,
        api::suppliers::create,
        api::suppliers::detail,
        api::suppliers::patch,
        api::suppliers::archive,
        api::suppliers::unarchive,
        api::manufacturers::list,
        api::manufacturers::create,
        api::manufacturers::detail,
        api::manufacturers::patch,
        api::manufacturers::archive,
        api::manufacturers::unarchive,
        api::drugs::list,
        api::drugs::create,
        api::drugs::detail,
        api::drugs::patch,
        api::drugs::archive,
        api::drugs::unarchive,
        api::drugs::list_packagings,
        api::drugs::create_packaging,
        api::drugs::patch_packaging,
        api::pricing::preview,
        api::lots::create_intake,
        api::lots::list,
        api::lots::detail,
        api::lots::create_correction,
        api::lots::correction_reasons,
        api::services::list,
        api::services::create,
        api::services::detail,
        api::services::patch,
        api::services::archive,
        api::services::unarchive,
        api::templates::list,
        api::templates::create,
        api::templates::detail,
        api::templates::patch,
        api::templates::archive,
        api::templates::unarchive,
        api::templates::list_items,
        api::templates::create_item,
        api::templates::patch_item,
        api::templates::delete_item,
        api::templates::move_item,
        api::picker::items,
        api::invoices::create_or_update,
        api::invoices::detail,
        api::invoices::pdf,
        api::invoices::accept,
        api::invoices::send,
        api::invoices::cancel,
        api::invoices::list,
        api::invoices::submit,
        api::invoices::bulk_submit,
        api::invoices::pending_pdfs,
        api::dashboard::get_dashboard,
        api::settings::get_settings,
        api::settings::patch_settings,
    ),
    components(schemas(error::ProblemDetails, error::FieldError)),
    tags(
        (name = "auth", description = "Login and session"),
        (name = "attachments", description = "Uploaded files, photos and generated PDFs"),
        (name = "appointments", description = "Dated visits holding treatments"),
        (name = "treatments", description = "Treatment records and their billing lines"),
        (name = "customers", description = "Customers, their addresses and contact data"),
        (name = "patients", description = "The practice's animals"),
        (name = "pharmacy", description = "Drugs, packagings, prices and stock"),
        (name = "services", description = "GOT positions and self-defined services"),
        (name = "templates", description = "Reusable ordered sets of treatment lines"),
        (name = "picker", description = "Unified drug/service search"),
        (name = "invoices", description = "Invoice lifecycle"),
        (name = "dashboard", description = "Landing dashboard"),
        (name = "settings", description = "Practice settings"),
    )
)]
pub struct ApiDoc;

/// Shared, cheap-to-clone application state.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub files: Arc<AttachmentStore>,
    pub mailer: Arc<Mailer>,
    pub login_throttle: Arc<LoginThrottle>,
}

impl AppState {
    /// Builds the shared state. Must be called from within the Tokio runtime — the SMTP
    /// transport starts its connection pool here.
    pub fn new(pool: PgPool, config: Arc<Config>) -> AppResult<Self> {
        let files = Arc::new(AttachmentStore::new(config.storage.attachments_dir.clone()));
        let mailer = Arc::new(Mailer::from_config(&config)?);
        Ok(Self {
            pool,
            config,
            files,
            mailer,
            login_throttle: Arc::new(LoginThrottle::default()),
        })
    }
}

/// Applies the database migrations bundled with the binary.
pub async fn run_migrations(pool: &PgPool) -> AppResult<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|error| AppError::internal("applying migrations", error))
}

/// Assembles the whole application: `/api`, health, metrics and the embedded frontend.
pub async fn build_app(state: AppState) -> AppResult<Router> {
    // Installed here so the very first request is already counted.
    metrics::install();

    let store = session_store::PostgresSessionStore::new(state.pool.clone());

    let session_layer = SessionManagerLayer::new(store)
        .with_name("kvet.sid")
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        // The Pi is reached over TLS in production; behind plain HTTP the cookie must still work.
        .with_secure(state.config.server.base_url.starts_with("https://"))
        .with_expiry(Expiry::OnInactivity(CookieDuration::days(
            SESSION_LIFETIME_DAYS,
        )));

    let upload_limit = state.config.max_upload_bytes();

    // Unknown /api paths answer with a problem document instead of the SPA's index.html,
    // and the auth middleware covers that fallback too — an anonymous caller never learns
    // which API paths exist.
    let protected = api::protected_routes()
        .fallback(api_not_found)
        .layer(middleware::from_fn(auth::require_auth));

    let api = Router::new().nest("/auth", auth::routes()).merge(protected);

    Ok(Router::new()
        .nest("/api", api)
        .route("/healthz", get(healthz))
        // Scraped locally by Prometheus; no session, like the health probe.
        .route("/metrics", get(metrics::scrape))
        .fallback(static_assets::handler)
        .layer(RequestBodyLimitLayer::new(upload_limit))
        .layer(session_layer)
        .layer(middleware::from_fn(metrics::track))
        .layer(TraceLayer::new_for_http())
        .with_state(state))
}

/// Liveness probe for systemd and the monitoring setup.
async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn api_not_found() -> AppError {
    AppError::NotFound
}
