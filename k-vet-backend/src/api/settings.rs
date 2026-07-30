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
    pub practice_street: String,
    pub practice_zip: String,
    pub practice_city: String,
    /// ISO 3166-1 alpha-2; empty falls back to `[invoice] default_country`.
    pub practice_country: Option<String>,
    /// Shown in the invoice footer. The sender address of outgoing mail is operator
    /// configuration and lives in `config.toml`.
    pub email: String,
    pub iban: String,
    pub bic: String,
    pub bank_name: String,
    pub ustid: String,
    pub logo_attachment_id: Option<i64>,
    /// Every outgoing invoice email is copied to these addresses.
    pub cc_emails: Vec<String>,
    pub bcc_emails: Vec<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchSettings {
    pub practice_name: Option<String>,
    pub practice_street: Option<String>,
    pub practice_zip: Option<String>,
    pub practice_city: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub practice_country: Option<Option<String>>,
    pub email: Option<String>,
    pub iban: Option<String>,
    pub bic: Option<String>,
    pub bank_name: Option<String>,
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
    let email = body
        .email
        .map(|value| match value.trim() {
            "" => Ok(String::new()),
            address => validate_email("email", address),
        })
        .transpose()?;
    let bic = body.bic.map(validate_bic).transpose()?;
    let country = match &body.practice_country {
        Some(Some(code)) => Some(Some(validate_country("practice_country", code)?)),
        other => other.clone(),
    };

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
             practice_street  = COALESCE($2, practice_street),
             practice_zip     = COALESCE($3, practice_zip),
             practice_city    = COALESCE($4, practice_city),
             practice_country = CASE WHEN $5 THEN $6 ELSE practice_country END,
             email            = COALESCE($7, email),
             iban             = COALESCE($8, iban),
             bic              = COALESCE($9, bic),
             bank_name        = COALESCE($10, bank_name),
             ustid            = COALESCE($11, ustid),
             logo_attachment_id = CASE WHEN $12 THEN $13 ELSE logo_attachment_id END,
             cc_emails        = COALESCE($14, cc_emails),
             bcc_emails       = COALESCE($15, bcc_emails)
         WHERE id",
        body.practice_name,
        body.practice_street,
        body.practice_zip,
        body.practice_city,
        country.is_some(),
        country.flatten(),
        email,
        body.iban,
        bic,
        body.bank_name,
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

/// A BIC is 8 or 11 alphanumeric characters (ISO 9362).
///
/// Worth rejecting rather than storing verbatim like the IBAN: the BIC also goes into the
/// GiroCode, where a malformed value produces a QR a banking app silently refuses.
fn validate_bic(value: String) -> AppResult<String> {
    let bic = value.trim().to_uppercase();
    if bic.is_empty() {
        return Ok(bic);
    }
    let shaped = matches!(bic.len(), 8 | 11) && bic.chars().all(|c| c.is_ascii_alphanumeric());
    if shaped {
        Ok(bic)
    } else {
        Err(AppError::field("bic", "value.invalidBic"))
    }
}

/// Validates a country against the real ISO 3166-1 list, not just its shape.
pub fn validate_country(field: &str, value: &str) -> AppResult<String> {
    let code = value.trim().to_uppercase();
    match rust_iso3166::from_alpha2(&code) {
        Some(_) => Ok(code),
        None => Err(AppError::field(field, "value.invalidCountry")),
    }
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
        "SELECT practice_name, practice_street, practice_zip, practice_city, practice_country,
                email, iban, bic, bank_name, ustid, logo_attachment_id, cc_emails, bcc_emails
         FROM global_settings WHERE id",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Settings {
        practice_name: row.practice_name,
        practice_street: row.practice_street,
        practice_zip: row.practice_zip,
        practice_city: row.practice_city,
        practice_country: row.practice_country,
        email: row.email,
        iban: row.iban,
        bic: row.bic,
        bank_name: row.bank_name,
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
