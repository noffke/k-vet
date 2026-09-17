//! The operator configuration the browser has to obey.
//!
//! `config.toml` decides the currency, the VAT rates the practice may charge and the country
//! to assume when none is given. The screens need all three — to offer the right VAT choices,
//! to format money, and to fill in a Land — and none of them belong in the database, where
//! the vet could edit them. This is the read-only counterpart to `/api/settings`, which is
//! the practice's own record.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::AppResult;

/// Every `vat_percent` column is `NUMERIC(7, 3)`, so `19` and `19.000` are the same rate but
/// not the same string — and the browser compares the strings.
const VAT_SCALE: u32 = 3;

#[derive(Debug, Serialize, ToSchema)]
pub struct OperatorConfig {
    /// ISO 4217, used for every amount the app formats.
    pub currency: String,
    /// The VAT rates a position may carry, in the order the operator listed them; the first
    /// is what a new record starts on. Scaled like the `vat_percent` columns, so a rate can
    /// be compared with a stored one as the string it is on the wire.
    #[schema(value_type = Vec<String>)]
    pub vat_rates: Vec<Decimal>,
    /// ISO 3166-1 alpha-2, filled into a country field that has none.
    pub default_country: String,
    /// The release this instance runs, from `KVET_VERSION` in the image; `dev` outside one.
    pub version: String,
    /// Set on every instance that is not the practice's own, and drawn as a banner. `None` on
    /// production.
    pub environment_label: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/config",
    operation_id = "getConfig",
    tag = "settings",
    responses((status = 200, description = "Operator configuration", body = OperatorConfig))
)]
pub async fn get_config(State(state): State<AppState>) -> AppResult<Json<OperatorConfig>> {
    let invoice = &state.config.invoice;
    let vat_rates = invoice
        .vat_rates
        .iter()
        .map(|rate| {
            let mut scaled = *rate;
            scaled.rescale(VAT_SCALE);
            scaled
        })
        .collect();
    Ok(Json(OperatorConfig {
        currency: invoice.currency.clone(),
        vat_rates,
        default_country: invoice.default_country.to_uppercase(),
        version: crate::version().to_owned(),
        // An empty string in the file means the same as no key at all: this is production.
        environment_label: state
            .config
            .server
            .environment_label
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .map(str::to_owned),
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/config", get(get_config))
}
