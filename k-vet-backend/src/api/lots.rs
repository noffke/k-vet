//! Stock: intakes, lots with derived remaining, batch traceability, corrections (T054).
//!
//! Stock is never stored as a number (FR-018). A lot snapshots how much arrived; what is
//! left is `initial_quantity + SUM(movements)` from the `lot_remaining` view, and every
//! change is a movement — dispenses tied to a treatment line, corrections with a reason.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::domain::enums::{InvoiceStatus, MovementKind};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Lot {
    pub id: i64,
    pub packaging_id: i64,
    pub drug_id: i64,
    pub drug_name: Option<String>,
    pub unit: Option<String>,
    pub arrival_date: NaiveDate,
    pub packages_received: i32,
    /// Snapshot of `packages × packaging quantity` at intake (FR-015).
    pub initial_quantity: Decimal,
    pub batch_number: Option<String>,
    pub expiration_date: Option<NaiveDate>,
    /// Derived, never stored.
    pub remaining: Decimal,
    pub created_at: DateTime<Utc>,
}

/// One line of a lot's history — a dispense with its links, or a correction with a reason.
#[derive(Debug, Serialize, ToSchema)]
pub struct Movement {
    pub id: i64,
    pub kind: MovementKind,
    /// Signed: dispenses are negative, corrections either way.
    pub quantity: Decimal,
    pub moved_at: DateTime<Utc>,
    pub reason: Option<String>,
    /// Set on the compensating correction of a cancelled invoice.
    pub reverses_movement_id: Option<i64>,
    pub treatment_id: Option<i64>,
    pub treatment_item_name: Option<String>,
    pub patient_id: Option<i64>,
    pub patient_name: Option<String>,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub invoice_id: Option<i64>,
    pub invoice_number: Option<String>,
    pub invoice_status: Option<InvoiceStatus>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LotDetail {
    #[serde(flatten)]
    pub lot: Lot,
    pub movements: Vec<Movement>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LotFilter {
    pub packaging_id: Option<i64>,
    pub drug_id: Option<i64>,
    /// Include lots that are used up (default: only lots with stock).
    pub empty: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateIntake {
    /// Defaults to today.
    pub arrival_date: Option<NaiveDate>,
    pub packages_received: i32,
    pub batch_number: Option<String>,
    pub expiration_date: Option<NaiveDate>,
}

/// A stocktake: the vet states what is physically there, the server books the difference.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCorrection {
    /// The counted remaining quantity. Defaults in the UI to the derived value.
    pub new_remaining: Decimal,
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/packagings/{id}/stock-intakes",
    operation_id = "createStockIntake",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = CreateIntake,
    responses(
        (status = 200, description = "The new lot", body = Lot),
        (status = 422, description = "Not an original packaging, or incomplete")
    )
)]
pub async fn create_intake(
    State(state): State<AppState>,
    Path(packaging_id): Path<i64>,
    Json(body): Json<CreateIntake>,
) -> AppResult<Json<Lot>> {
    if body.packages_received <= 0 {
        return Err(AppError::field("packages_received", "value.mustBePositive"));
    }

    let packaging = sqlx::query!(
        "SELECT quantity, draft FROM drug_packaging WHERE id = $1 AND kind = 'original'",
        packaging_id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::field("packaging_id", "lot.originalPackagingOnly"))?;

    if packaging.draft {
        return Err(AppError::field("packaging_id", "record.incomplete"));
    }
    let quantity = packaging
        .quantity
        .ok_or_else(|| AppError::field("packaging_id", "record.incomplete"))?;

    // The initial quantity is snapshotted here: a later change of the packaging size must
    // not rewrite what physically arrived.
    let initial_quantity = quantity * Decimal::from(body.packages_received);
    let arrival_date = body
        .arrival_date
        .unwrap_or_else(|| chrono::Local::now().date_naive());

    let id: i64 = sqlx::query_scalar!(
        r#"INSERT INTO drug_stock_lot
               (packaging_id, packaging_kind, arrival_date, packages_received,
                initial_quantity, batch_number, expiration_date)
           VALUES ($1, 'original', $2, $3, $4, $5, $6)
           RETURNING id"#,
        packaging_id,
        arrival_date,
        body.packages_received,
        initial_quantity,
        body.batch_number,
        body.expiration_date,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(load_lot(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/lots",
    operation_id = "listLots",
    tag = "pharmacy",
    params(LotFilter),
    responses((status = 200, body = Vec<Lot>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(filter): Query<LotFilter>,
) -> AppResult<Json<Vec<Lot>>> {
    let rows = sqlx::query!(
        r#"SELECT lot.id
           FROM drug_stock_lot lot
           JOIN lot_remaining remaining ON remaining.lot_id = lot.id
           JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
           WHERE ($1::bigint IS NULL OR lot.packaging_id = $1)
             AND ($2::bigint IS NULL OR packaging.drug_id = $2)
             AND ($3 OR remaining.remaining <> 0)
           ORDER BY lot.expiration_date ASC NULLS LAST, lot.id"#,
        filter.packaging_id,
        filter.drug_id,
        filter.empty.unwrap_or(false),
    )
    .fetch_all(&state.pool)
    .await?;

    let mut lots = Vec::with_capacity(rows.len());
    for row in rows {
        lots.push(load_lot(&state.pool, row.id).await?);
    }
    Ok(Json(lots))
}

#[utoipa::path(
    get,
    path = "/api/lots/{id}",
    operation_id = "getLot",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = LotDetail), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<LotDetail>> {
    let lot = load_lot(&state.pool, id).await?;

    // Batch traceability (FR-017, SC-004): every movement in order, dispenses linked to
    // the treatment, invoice, customer and patient that received them.
    let movements = sqlx::query!(
        r#"SELECT movement.id, movement.kind AS "kind: MovementKind", movement.quantity,
                  movement.moved_at, movement.reason, movement.reverses_movement_id,
                  item.treatment_id AS "treatment_id?", item.name AS "treatment_item_name?",
                  patient.id AS "patient_id?", patient.name AS "patient_name?",
                  customer.id AS "customer_id?", customer.first_name AS "customer_first_name?",
                  customer.last_name AS "customer_last_name?",
                  invoice.id AS "invoice_id?", invoice.invoice_number AS "invoice_number?",
                  invoice.status AS "invoice_status?: InvoiceStatus"
           FROM drug_stock_movement movement
           LEFT JOIN treatment_item item ON item.id = movement.treatment_item_id
           LEFT JOIN patient ON patient.id = item.patient_id
           LEFT JOIN customer ON customer.id = patient.customer_id
           LEFT JOIN invoice ON invoice.treatment_id = item.treatment_id
                            AND invoice.status <> 'cancelled'
           WHERE movement.lot_id = $1
           ORDER BY movement.moved_at, movement.id"#,
        id,
    )
    .fetch_all(&state.pool)
    .await?;

    let movements = movements
        .into_iter()
        .map(|row| Movement {
            customer_name: match (&row.customer_first_name, &row.customer_last_name) {
                (None, None) => None,
                (first, last) => Some(
                    [first.clone(), last.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            },
            id: row.id,
            kind: row.kind,
            quantity: row.quantity,
            moved_at: row.moved_at,
            reason: row.reason,
            reverses_movement_id: row.reverses_movement_id,
            treatment_id: row.treatment_id,
            treatment_item_name: row.treatment_item_name,
            patient_id: row.patient_id,
            patient_name: row.patient_name,
            customer_id: row.customer_id,
            invoice_id: row.invoice_id,
            invoice_number: row.invoice_number,
            invoice_status: row.invoice_status,
        })
        .collect();

    Ok(Json(LotDetail { lot, movements }))
}

#[utoipa::path(
    post,
    path = "/api/lots/{id}/corrections",
    operation_id = "createCorrection",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = CreateCorrection,
    responses(
        (status = 200, body = LotDetail),
        (status = 422, description = "The correction would not change anything")
    )
)]
pub async fn create_correction(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CreateCorrection>,
) -> AppResult<Json<LotDetail>> {
    let remaining: Option<Decimal> =
        sqlx::query_scalar!("SELECT remaining FROM lot_remaining WHERE lot_id = $1", id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?;
    let remaining = remaining.unwrap_or_default();

    // The vet states the counted stock; the ledger records the difference.
    let delta = body.new_remaining - remaining;
    if delta.is_zero() {
        return Err(AppError::field("new_remaining", "value.mustNotBeZero"));
    }

    let reason = body
        .reason
        .map(|reason| reason.trim().to_owned())
        .filter(|reason| !reason.is_empty());

    sqlx::query!(
        "INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason)
         VALUES ($1, 'correction', $2, $3)",
        id,
        delta,
        reason,
    )
    .execute(&state.pool)
    .await?;

    detail(State(state), Path(id)).await
}

#[utoipa::path(
    get,
    path = "/api/stock/correction-reasons",
    operation_id = "listCorrectionReasons",
    tag = "pharmacy",
    responses((status = 200, description = "Previously used reasons, alphabetical", body = Vec<String>))
)]
pub async fn correction_reasons(State(state): State<AppState>) -> AppResult<Json<Vec<String>>> {
    // An editable select: the reasons the practice already used, plus free text (FR-016).
    let reasons = sqlx::query_scalar!(
        "SELECT DISTINCT reason FROM drug_stock_movement
         WHERE kind = 'correction' AND reason IS NOT NULL AND reason <> ''
         ORDER BY reason",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(reasons.into_iter().flatten().collect()))
}

async fn load_lot(pool: &sqlx::PgPool, id: i64) -> AppResult<Lot> {
    let row = sqlx::query!(
        r#"SELECT lot.id, lot.packaging_id, lot.arrival_date, lot.packages_received,
                  lot.initial_quantity, lot.batch_number, lot.expiration_date, lot.created_at,
                  packaging.drug_id, packaging.unit, drug.name AS "drug_name?",
                  remaining.remaining AS "remaining?"
           FROM drug_stock_lot lot
           JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
           JOIN drug ON drug.id = packaging.drug_id
           LEFT JOIN lot_remaining remaining ON remaining.lot_id = lot.id
           WHERE lot.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Lot {
        id: row.id,
        packaging_id: row.packaging_id,
        drug_id: row.drug_id,
        drug_name: row.drug_name,
        unit: row.unit,
        arrival_date: row.arrival_date,
        packages_received: row.packages_received,
        initial_quantity: row.initial_quantity,
        batch_number: row.batch_number,
        expiration_date: row.expiration_date,
        remaining: row.remaining.unwrap_or_default(),
        created_at: row.created_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/packagings/{id}/stock-intakes", post(create_intake))
        .route("/lots", get(list))
        .route("/lots/{id}", get(detail))
        .route("/lots/{id}/corrections", post(create_correction))
        .route("/stock/correction-reasons", get(correction_reasons))
}
