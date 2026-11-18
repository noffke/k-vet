//! Stock: intakes, lots with derived remaining, batch traceability, corrections (T054).
//!
//! Stock is never stored as a number (FR-018). A lot snapshots how much arrived; what is
//! left is `initial_quantity + SUM(movements)` from the `lot_remaining` view, and every
//! change is a movement — dispenses tied to a treatment line, corrections with a reason.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::api::drugs;
use crate::domain::enums::{InvoiceStatus, MovementKind};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Lot {
    pub id: i64,
    pub packaging_id: i64,
    pub drug_id: i64,
    pub drug_name: Option<String>,
    /// The drug is "Ohne Bestand" now (migration 0043): a lot left from before the flag stays
    /// readable as history, and takes no correction.
    pub drug_without_stock: bool,
    pub unit: Option<String>,
    pub arrival_date: NaiveDate,
    pub packages_received: i32,
    /// The packaging's size at the time of reading — what one package holds. The lot row
    /// says "2 Packungen à 100 ml", which needs the size as well as the count.
    pub packaging_quantity: Option<Decimal>,
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
    /// The original's list price as the vet confirmed it at this delivery — a changed
    /// wholesaler price is easily missed. When it differs from the stored one, the stored one
    /// changes together with the lot, and with it, per AMPreisV, the sales prices of the original
    /// and its subsets (unless set by hand). Lines already billed keep their prices.
    pub list_price_net: Option<Decimal>,
}

/// A correction of a lot's stock, in one of two ways — exactly one of them:
/// a stocktake, where the vet states what is physically there and the server books the
/// difference; or a change, where she states the difference itself — one broken bottle is
/// −50 ml, a pack found again +50 ml — without counting the rest (issues.md, 2026-11-18).
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCorrection {
    /// The counted remaining quantity. Defaults in the UI to the derived value.
    pub new_remaining: Option<Decimal>,
    /// The change, signed, in the lot's base units.
    pub delta: Option<Decimal>,
    pub reason: Option<String>,
}

/// What the vet always finds among the reasons, beside the ones she used before: the two a
/// change is usually for.
const STANDARD_REASONS: [&str; 2] = ["Ablauf", "Verlust"];

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
    if body
        .list_price_net
        .is_some_and(|price| price <= Decimal::ZERO)
    {
        return Err(AppError::field("list_price_net", "value.mustBePositive"));
    }

    let mut transaction = state.pool.begin().await?;
    let packaging = sqlx::query!(
        "SELECT drug_id, quantity, list_price_net, draft FROM drug_packaging
         WHERE id = $1 AND kind = 'original'
         FOR UPDATE",
        packaging_id,
    )
    .fetch_optional(&mut *transaction)
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
    let batch_number = body
        .batch_number
        .as_deref()
        .map(str::trim)
        .filter(|batch| !batch.is_empty());

    if let Some(price) = body.list_price_net
        && packaging.list_price_net != Some(price)
    {
        sqlx::query!(
            "UPDATE drug_packaging SET list_price_net = $2 WHERE id = $1",
            packaging_id,
            price,
        )
        .execute(&mut *transaction)
        .await?;
        drugs::recompute_prices(&mut transaction, packaging.drug_id, &state.config.pharmacy)
            .await?;
    }

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
        batch_number,
        body.expiration_date,
    )
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

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
           -- The animal a batch went to is the line's owner; the movement has no patient
           -- of its own, which is why the ownership had to be structural.
           LEFT JOIN patient_treatment record ON record.id = item.patient_treatment_id
           LEFT JOIN patient ON patient.id = record.patient_id
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
        (status = 422, description = "Neither or both of count and change, no change at all, below zero, or a drug \"Ohne Bestand\"")
    )
)]
pub async fn create_correction(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CreateCorrection>,
) -> AppResult<Json<LotDetail>> {
    // A count, or a change: exactly one. Each is refused on its own field.
    let (field, counted, change) = match (body.new_remaining, body.delta) {
        (Some(counted), None) => ("new_remaining", Some(counted), None),
        (None, Some(change)) => ("delta", None, Some(change)),
        _ => return Err(AppError::field("delta", "correction.countOrChange")),
    };
    // A count of what is physically there cannot be negative (FR-018, issues.md 8). Caught
    // here rather than left to the trigger, so it reads as the field error it is.
    if counted.is_some_and(|counted| counted.is_sign_negative()) {
        return Err(AppError::field("new_remaining", "value.mustNotBeNegative"));
    }
    if change.is_some_and(|change| change.is_zero()) {
        return Err(AppError::field("delta", "value.mustNotBeZero"));
    }

    // One transaction, and the lot locked: the stocktake is a read of the current stock
    // followed by a write of the difference, and a dispense landing in between would otherwise
    // be silently undone by the delta computed before it. A change is booked as stated, so it
    // stays a change even then; the lock keeps its check against zero true.
    let mut transaction = state.pool.begin().await?;
    sqlx::query_scalar!("SELECT id FROM drug_stock_lot WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;

    // A consumable "Ohne Bestand" keeps no stock, so there is nothing to correct — also on a
    // lot left from before the flag (the database refuses the movement too).
    let without_stock: bool = sqlx::query_scalar!(
        "SELECT drug.without_stock
         FROM drug_stock_lot lot
         JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
         JOIN drug ON drug.id = packaging.drug_id
         WHERE lot.id = $1",
        id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    if without_stock {
        return Err(AppError::field(field, "lot.drugWithoutStock"));
    }

    let remaining: Option<Decimal> =
        sqlx::query_scalar!("SELECT remaining FROM lot_remaining WHERE lot_id = $1", id)
            .fetch_one(&mut *transaction)
            .await?;
    let remaining = remaining.unwrap_or_default();

    // The ledger records a difference either way: the one stated, or the count's against the
    // books.
    let delta = match (counted, change) {
        (_, Some(change)) => {
            // Said on the field typed into, rather than by the trigger, which names the count.
            if remaining + change < Decimal::ZERO {
                return Err(AppError::field("delta", "movement.wouldGoNegative"));
            }
            change
        }
        (Some(counted), None) => counted - remaining,
        (None, None) => return Err(AppError::field("delta", "correction.countOrChange")),
    };
    if delta.is_zero() {
        return Err(AppError::field(field, "value.mustNotBeZero"));
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
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

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
    // An editable select: the reasons the practice already used, plus free text (FR-016) —
    // and always "Ablauf" and "Verlust", what a change is usually for.
    let used = sqlx::query_scalar!(
        "SELECT DISTINCT reason FROM drug_stock_movement
         WHERE kind = 'correction' AND reason IS NOT NULL AND reason <> ''",
    )
    .fetch_all(&state.pool)
    .await?;
    let reasons: std::collections::BTreeSet<String> = used
        .into_iter()
        .flatten()
        .chain(STANDARD_REASONS.iter().map(|reason| (*reason).to_owned()))
        .collect();
    Ok(Json(reasons.into_iter().collect()))
}

async fn load_lot(pool: &sqlx::PgPool, id: i64) -> AppResult<Lot> {
    let row = sqlx::query!(
        r#"SELECT lot.id, lot.packaging_id, lot.arrival_date, lot.packages_received,
                  lot.initial_quantity, lot.batch_number, lot.expiration_date, lot.created_at,
                  packaging.drug_id, packaging.unit, packaging.quantity AS "packaging_quantity?",
                  drug.name AS "drug_name?", drug.without_stock,
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
        drug_without_stock: row.without_stock,
        unit: row.unit,
        arrival_date: row.arrival_date,
        packages_received: row.packages_received,
        packaging_quantity: row.packaging_quantity,
        initial_quantity: row.initial_quantity,
        batch_number: row.batch_number,
        expiration_date: row.expiration_date,
        remaining: row.remaining.unwrap_or_default(),
        created_at: row.created_at,
    })
}

/// Takes back a lot booked by mistake — a wrong count, the wrong drug — while it is untouched:
/// nothing dispensed from it, nothing corrected. Once a movement refers to it, the lot is part of
/// the stock history and stays; a stocktake correction is then the way to put it right.
#[utoipa::path(
    delete,
    path = "/api/lots/{id}",
    operation_id = "deleteLot",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses(
        (status = 204, description = "Taken back"),
        (status = 404),
        (status = 409, description = "Something was dispensed from it or corrected")
    )
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let mut transaction = state.pool.begin().await?;
    sqlx::query_scalar!("SELECT id FROM drug_stock_lot WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    let touched: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM drug_stock_movement WHERE lot_id = $1)",
        id
    )
    .fetch_one(&mut *transaction)
    .await?;
    if touched.unwrap_or(true) {
        return Err(AppError::Conflict(
            "the lot has movements and stays".to_owned(),
        ));
    }
    sqlx::query!("DELETE FROM drug_stock_lot WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/packagings/{id}/stock-intakes", post(create_intake))
        .route("/lots", get(list))
        .route("/lots/{id}", get(detail).delete(delete))
        .route("/lots/{id}/corrections", post(create_correction))
        .route("/stock/correction-reasons", get(correction_reasons))
}
