//! The invoice mail queue (migration 0026): what is waiting to go out, and what failed.
//!
//! No request talks to a mail server. Releasing an invoice or "E-Mail senden" only puts the mail
//! here; `jobs::send_queued_mail` tries what is waiting every five minutes, keeps the error on
//! the row, retries for [`RETRY_HOURS`] after the first failure and stops at once when the mail
//! server refuses for good. The dashboard lists the failures, to retry or remove.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::{AppError, AppResult};

/// How long a failing mail is retried, counted from its first failure.
pub const RETRY_HOURS: i32 = 6;

/// A mail waiting in the queue — or failing there.
#[derive(Debug, Serialize, ToSchema)]
pub struct QueuedEmail {
    pub id: i64,
    pub invoice_id: i64,
    pub recipients: Vec<String>,
    pub queued_at: DateTime<Utc>,
    /// Not sent before this: now for "send now", a chosen hour for "send later".
    pub send_after: DateTime<Utc>,
    pub attempts: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// The transport's own words for the last failure: the host it could not reach, or the
    /// mail server's reply ("permanent error (550): no such user"). Empty while nothing failed.
    pub last_error: Option<String>,
    /// The last failure was final — the mail server refused for good, or the mail could not be
    /// built — so it is not retried.
    pub refused: bool,
    /// Until when a failing mail is retried; empty while nothing failed.
    pub retry_until: Option<DateTime<Utc>>,
    /// No more attempts: refused, or still failing [`RETRY_HOURS`] after the first failure.
    pub gave_up_at: Option<DateTime<Utc>>,
}

/// Queues the invoice's mail to `recipients`, to go out with the first run at or after
/// `send_after`. A mail already waiting for the invoice — scheduled, failing or given up — is
/// replaced, with a fresh start: there is only ever one per invoice.
pub async fn queue(
    connection: &mut PgConnection,
    invoice_id: i64,
    recipients: &[String],
    send_after: DateTime<Utc>,
) -> AppResult<()> {
    sqlx::query!(
        "INSERT INTO mail_queue (invoice_id, recipients, send_after) VALUES ($1, $2, $3)
         ON CONFLICT (invoice_id) DO UPDATE
            SET recipients = EXCLUDED.recipients, send_after = EXCLUDED.send_after,
                queued_at = now(), attempts = 0, last_attempt_at = NULL,
                first_failure_at = NULL, last_error = NULL, refused = false, gave_up_at = NULL",
        invoice_id,
        recipients,
        send_after,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Drops the invoice's waiting mail: a cancelled invoice must not reach the customer, and one
/// handed over on paper does not need to arrive twice.
pub async fn drop_for_invoice(connection: &mut PgConnection, invoice_id: i64) -> AppResult<()> {
    sqlx::query!("DELETE FROM mail_queue WHERE invoice_id = $1", invoice_id)
        .execute(connection)
        .await?;
    Ok(())
}

/// The invoice's queued mail, if one is waiting or failing.
pub async fn for_invoice(
    connection: &mut PgConnection,
    invoice_id: i64,
) -> AppResult<Option<QueuedEmail>> {
    Ok(sqlx::query_as!(
        QueuedEmail,
        r#"SELECT id, invoice_id, recipients, queued_at, send_after, attempts, last_attempt_at,
                  last_error, refused, gave_up_at,
                  first_failure_at + make_interval(hours => $2) AS retry_until
           FROM mail_queue WHERE invoice_id = $1"#,
        invoice_id,
        RETRY_HOURS,
    )
    .fetch_optional(connection)
    .await?)
}

/// Every mail that failed, given up or still being retried, the oldest failure first.
pub async fn failed(connection: &mut PgConnection) -> AppResult<Vec<QueuedEmail>> {
    Ok(sqlx::query_as!(
        QueuedEmail,
        r#"SELECT id, invoice_id, recipients, queued_at, send_after, attempts, last_attempt_at,
                  last_error, refused, gave_up_at,
                  first_failure_at + make_interval(hours => $1) AS retry_until
           FROM mail_queue WHERE last_error IS NOT NULL
           ORDER BY first_failure_at, id"#,
        RETRY_HOURS,
    )
    .fetch_all(connection)
    .await?)
}

#[utoipa::path(
    post,
    operation_id = "retryQueuedMail",
    path = "/api/mail-queue/{id}/retry",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses((status = 200, body = QueuedEmail), (status = 404))
)]
/// Tries a failed mail again from the start, and now: the next run of the queue (within five
/// minutes) sends it, and a failure gets a fresh [`RETRY_HOURS`].
pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<QueuedEmail>> {
    let mut connection = state.pool.acquire().await?;
    let invoice_id = sqlx::query_scalar!(
        "UPDATE mail_queue
            SET queued_at = now(), send_after = now(), attempts = 0, last_attempt_at = NULL,
                first_failure_at = NULL, last_error = NULL, refused = false, gave_up_at = NULL
          WHERE id = $1
         RETURNING invoice_id",
        id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;
    for_invoice(&mut connection, invoice_id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

#[utoipa::path(
    delete,
    operation_id = "removeQueuedMail",
    path = "/api/mail-queue/{id}",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 404))
)]
/// Takes a mail out of the queue. The invoice stays released and unsent: printed and posted,
/// or emailed again, it moves on.
pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let removed = sqlx::query!("DELETE FROM mail_queue WHERE id = $1", id)
        .execute(&state.pool)
        .await?;
    if removed.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/mail-queue/{id}/retry", post(retry))
        .route("/mail-queue/{id}", delete(remove))
}
