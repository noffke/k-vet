//! The landing dashboard (T072).
//!
//! Everything here is work the vet would otherwise have to go looking for (FR-036): the money
//! that has not arrived yet, what is still waiting for the bookkeeper, and which stock is about
//! to expire while it is still worth something.
//!
//! Three ways a treated animal never turns into paid work, and each is its own list because
//! each has its own remedy: the treatment was never billed, the invoice was never released, or
//! the invoice was released and never reached the customer.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::AppState;
use crate::api::{invoices, treatment_items, treatments};
use crate::domain::enums::InvoiceStatus;
use crate::error::AppResult;

/// How many expiring lots the dashboard shows before it stops being a glance.
const EXPIRING_LOTS: i64 = 5;
/// The same for each money-at-risk list; the count says how many more there are.
const AT_RISK_ROWS: i64 = 5;

#[derive(Debug, Serialize, ToSchema)]
pub struct Dashboard {
    /// Sent invoices not yet handed over to bookkeeping.
    pub pending_invoice_count: i64,
    /// Treatments that were worked and billed to nobody.
    pub unbilled: AtRisk,
    /// Invoices written but never released.
    pub unreleased: AtRisk,
    /// Invoices released but never sent to the customer — including the ones whose email
    /// failed on the way out, which is the case nothing used to show.
    pub unsent: AtRisk,
    /// The lots expiring soonest that still hold stock.
    pub expiring_lots: Vec<ExpiringLot>,
}

/// One money-at-risk list: the oldest few cases, and how many there are in all.
#[derive(Debug, Serialize, ToSchema)]
pub struct AtRisk {
    pub count: i64,
    pub entries: Vec<AtRiskEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AtRiskEntry {
    /// Every case is resolved on the treatment page, so that is where a row leads.
    pub treatment_id: i64,
    /// Absent while no invoice exists.
    pub invoice_number: Option<String>,
    /// The invoice's date, or the visit's while there is no invoice.
    pub date: Option<NaiveDate>,
    pub customer_name: String,
    pub patients: Vec<String>,
    #[schema(value_type = String)]
    pub total_gross: Decimal,
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
        sqlx::query_scalar!("SELECT count(*) FROM invoice WHERE status = 'sent'")
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
        unbilled: unbilled_treatments(&state.pool).await?,
        unreleased: invoices_at_risk(&state.pool, InvoiceStatus::Created).await?,
        unsent: invoices_at_risk(&state.pool, InvoiceStatus::Accepted).await?,
        expiring_lots,
    }))
}

/// Treatments that carry positions and no live invoice — worked and billed to nobody.
///
/// Only visits before today count. A treatment being written up right now has positions and no
/// invoice yet by definition, and a dashboard that shouts about the work in hand teaches the
/// vet to ignore it.
async fn unbilled_treatments(pool: &PgPool) -> AppResult<AtRisk> {
    let count = sqlx::query_scalar!(
        "SELECT count(*)
         FROM treatment
         JOIN appointment ON appointment.id = treatment.appointment_id
         WHERE appointment.starts_at < date_trunc('day', now())
           AND EXISTS (
                 SELECT 1 FROM treatment_item WHERE treatment_item.treatment_id = treatment.id)
           AND NOT EXISTS (
                 SELECT 1 FROM invoice
                 WHERE invoice.treatment_id = treatment.id AND invoice.status <> 'cancelled')",
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    let rows = sqlx::query!(
        r#"SELECT treatment.id, appointment.starts_at::date AS "visit_date?"
           FROM treatment
           JOIN appointment ON appointment.id = treatment.appointment_id
           WHERE appointment.starts_at < date_trunc('day', now())
             AND EXISTS (
                   SELECT 1 FROM treatment_item WHERE treatment_item.treatment_id = treatment.id)
             AND NOT EXISTS (
                   SELECT 1 FROM invoice
                   WHERE invoice.treatment_id = treatment.id AND invoice.status <> 'cancelled')
           ORDER BY appointment.starts_at, treatment.id
           LIMIT $1"#,
        AT_RISK_ROWS,
    )
    .fetch_all(pool)
    .await?;

    let mut connection = pool.acquire().await?;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let parties = treatments::parties(pool, row.id).await?;
        entries.push(AtRiskEntry {
            treatment_id: row.id,
            invoice_number: None,
            date: row.visit_date,
            customer_name: parties.customer_name,
            patients: parties.patients,
            total_gross: treatment_items::treatment_total_gross(&mut connection, row.id).await?,
        });
    }

    Ok(AtRisk { count, entries })
}

/// Invoices stuck in one stage, oldest first — the longer one sits there the worse it is.
async fn invoices_at_risk(pool: &PgPool, status: InvoiceStatus) -> AppResult<AtRisk> {
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) FROM invoice WHERE status = $1"#,
        status as InvoiceStatus,
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    let ids = sqlx::query_scalar!(
        r#"SELECT id FROM invoice WHERE status = $1
           ORDER BY invoice_date, id
           LIMIT $2"#,
        status as InvoiceStatus,
        AT_RISK_ROWS,
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::with_capacity(ids.len());
    for id in ids {
        let invoice = invoices::load(pool, id).await?;
        entries.push(AtRiskEntry {
            treatment_id: invoice.treatment_id,
            invoice_number: Some(invoice.invoice_number),
            date: Some(invoice.invoice_date),
            customer_name: invoice.customer_name,
            patients: invoice.patients,
            total_gross: invoice.total_gross,
        });
    }

    Ok(AtRisk { count, entries })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/dashboard", get(get_dashboard))
}
