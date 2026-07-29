//! Practice settings (T072).
//!
//! One row, edited field by field like every other record in the app (FR-037). What lives
//! here is what the practice itself owns — name, address, IBAN, UStID, logo and the global
//! copy recipients. Mail server, invoice number pattern and currency stay operator
//! configuration and are deliberately not editable.

use axum::extract::State;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::double_option;
use crate::domain::contact::validate_email;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Settings {
    pub practice_name: String,
    /// Printed on the invoice as written, line breaks included.
    pub practice_address: String,
    pub iban: String,
    pub ustid: String,
    pub logo_attachment_id: Option<i64>,
    /// Every outgoing invoice email is copied to these addresses.
    pub cc_emails: Vec<String>,
    pub bcc_emails: Vec<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchSettings {
    pub practice_name: Option<String>,
    pub practice_address: Option<String>,
    pub iban: Option<String>,
    pub ustid: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<i64>)]
    pub logo_attachment_id: Option<Option<i64>>,
    pub cc_emails: Option<Vec<String>>,
    pub bcc_emails: Option<Vec<String>>,
}

#[utoipa::path(
    get,
    operation_id = "getSettings",
    path = "/api/settings",
    tag = "settings",
    responses((status = 200, body = Settings))
)]
pub async fn get_settings(State(state): State<AppState>) -> AppResult<Json<Settings>> {
    load(&state).await.map(Json)
}

#[utoipa::path(
    patch,
    operation_id = "patchSettings",
    path = "/api/settings",
    tag = "settings",
    request_body = PatchSettings,
    responses(
        (status = 200, body = Settings),
        (status = 422, description = "An address is malformed, or the logo does not exist")
    )
)]
pub async fn patch_settings(
    State(state): State<AppState>,
    Json(body): Json<PatchSettings>,
) -> AppResult<Json<Settings>> {
    let cc_emails = validate_all("cc_emails", body.cc_emails)?;
    let bcc_emails = validate_all("bcc_emails", body.bcc_emails)?;

    if let Some(Some(attachment_id)) = body.logo_attachment_id {
        let exists =
            sqlx::query_scalar!("SELECT true FROM attachment WHERE id = $1", attachment_id)
                .fetch_optional(&state.pool)
                .await?;
        if exists.is_none() {
            return Err(AppError::field("logo_attachment_id", "error.notFound"));
        }
    }

    sqlx::query!(
        "UPDATE global_settings
         SET practice_name    = COALESCE($1, practice_name),
             practice_address = COALESCE($2, practice_address),
             iban             = COALESCE($3, iban),
             ustid            = COALESCE($4, ustid),
             logo_attachment_id = CASE WHEN $5 THEN $6 ELSE logo_attachment_id END,
             cc_emails        = COALESCE($7, cc_emails),
             bcc_emails       = COALESCE($8, bcc_emails)
         WHERE id",
        body.practice_name,
        body.practice_address,
        body.iban,
        body.ustid,
        body.logo_attachment_id.is_some(),
        body.logo_attachment_id.flatten(),
        cc_emails.as_deref(),
        bcc_emails.as_deref(),
    )
    .execute(&state.pool)
    .await?;

    load(&state).await.map(Json)
}

/// Validates a list of addresses, reporting the list's own field name.
fn validate_all(field: &str, emails: Option<Vec<String>>) -> AppResult<Option<Vec<String>>> {
    emails
        .map(|entries| {
            entries
                .iter()
                .map(|email| validate_email(field, email))
                .collect::<AppResult<Vec<String>>>()
        })
        .transpose()
}

async fn load(state: &AppState) -> AppResult<Settings> {
    let row = sqlx::query!(
        "SELECT practice_name, practice_address, iban, ustid, logo_attachment_id,
                cc_emails, bcc_emails
         FROM global_settings WHERE id",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Settings {
        practice_name: row.practice_name,
        practice_address: row.practice_address,
        iban: row.iban,
        ustid: row.ustid,
        logo_attachment_id: row.logo_attachment_id,
        cc_emails: row.cc_emails,
        bcc_emails: row.bcc_emails,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", patch(patch_settings))
}
