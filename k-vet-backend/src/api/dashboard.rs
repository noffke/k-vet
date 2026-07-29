//! The landing dashboard (T072).
//!
//! Two questions get answered here, both of them work the vet would otherwise have to go
//! looking for (FR-036): what is still waiting for the bookkeeper, and which stock is about
//! to expire while it is still worth something.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::AppResult;

/// How many expiring lots the dashboard shows before it stops being a glance.
const EXPIRING_LOTS: i64 = 5;

#[derive(Debug, Serialize, ToSchema)]
pub struct Dashboard {
    /// Accepted invoices not yet handed over to bookkeeping.
    pub pending_invoice_count: i64,
    /// The lots expiring soonest that still hold stock.
    pub expiring_lots: Vec<ExpiringLot>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExpiringLot {
    pub lot_id: i64,
    pub drug_id: i64,
    pub drug_name: String,
    pub batch_number: Option<String>,
    pub expiration_date: NaiveDate,
    /// Remaining base units, from the `lot_remaining` view.
    #[schema(value_type = String)]
    pub remaining: Decimal,
    pub unit: Option<String>,
}

#[utoipa::path(
    get,
    operation_id = "getDashboard",
    path = "/api/dashboard",
    tag = "dashboard",
    responses((status = 200, body = Dashboard))
)]
pub async fn get_dashboard(State(state): State<AppState>) -> AppResult<Json<Dashboard>> {
    let pending_invoice_count =
        sqlx::query_scalar!("SELECT count(*) FROM invoice WHERE status = 'accepted'")
            .fetch_one(&state.pool)
            .await?
            .unwrap_or(0);

    // Undated lots cannot expire, and an empty lot is nothing left to lose.
    let rows = sqlx::query!(
        r#"SELECT lot.lot_id AS "lot_id!", lot.drug_id AS "drug_id!",
                  drug.name AS "drug_name?", lot.batch_number,
                  lot.expiration_date AS "expiration_date!", lot.remaining AS "remaining!",
                  packaging.unit
           FROM lot_remaining lot
           JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
           JOIN drug ON drug.id = lot.drug_id
           WHERE lot.expiration_date IS NOT NULL
             AND lot.remaining > 0
             AND NOT drug.archived
           ORDER BY lot.expiration_date, lot.lot_id
           LIMIT $1"#,
        EXPIRING_LOTS,
    )
    .fetch_all(&state.pool)
    .await?;

    let expiring_lots = rows
        .into_iter()
        .map(|row| ExpiringLot {
            lot_id: row.lot_id,
            drug_id: row.drug_id,
            drug_name: row.drug_name.unwrap_or_default(),
            batch_number: row.batch_number,
            expiration_date: row.expiration_date,
            remaining: row.remaining,
            unit: row.unit,
        })
        .collect();

    Ok(Json(Dashboard {
        pending_invoice_count,
        expiring_lots,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/dashboard", get(get_dashboard))
}
