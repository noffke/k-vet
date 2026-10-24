//! The invoice email templates the vet chooses from when releasing or sending an invoice.
//!
//! Configured in `config.toml` (`[[invoice.email_templates]]`), each a file in the templates
//! folder; the first is the default. Whether a file is there is looked at on every call, so the
//! form shows the folder as it is now — a template whose file is missing is offered disabled,
//! and becomes selectable as soon as the file is put in place.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::AppResult;

#[derive(Debug, Serialize, ToSchema)]
pub struct EmailTemplate {
    /// What the vet sees, and what a release or send names.
    pub name: String,
    /// The file in the templates folder — said when it is missing, so it can be put back.
    pub file: String,
    /// The file is there and readable; a template without one cannot be chosen.
    pub available: bool,
}

#[utoipa::path(
    get,
    path = "/api/email-templates",
    operation_id = "listEmailTemplates",
    tag = "invoices",
    responses((status = 200, description = "The configured templates, the default first", body = Vec<EmailTemplate>))
)]
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<EmailTemplate>>> {
    Ok(Json(
        state
            .mailer
            .templates()
            .into_iter()
            .map(|template| EmailTemplate {
                name: template.name,
                file: template.file,
                available: template.available,
            })
            .collect(),
    ))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/email-templates", get(list))
}
