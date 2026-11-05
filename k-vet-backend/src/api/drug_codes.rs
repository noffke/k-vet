//! The codes on a drug's packs (migration 0038), and what a scan names.
//!
//! A barcode scanner types what it reads; `POST /api/scan` turns that into the product code, the
//! batch and expiry a Data Matrix may carry, and the drug the code belongs to — the first step of
//! every Wareneingang by scanner. Reading changes nothing; attaching a code is its own request.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::drugs::{self, Drug, Packaging};
use crate::domain::barcode;
use crate::domain::enums::PackagingKind;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct DrugCode {
    pub id: i64,
    /// 14 digits; a PZN is kept as its NTIN (`04150` + PZN + check digit).
    pub gtin: String,
    /// As people know it: "PZN 12345678", or the EAN-13.
    pub display: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddDrugCode {
    /// What was scanned or typed: an EAN, a PZN, a whole Data Matrix. Only the product code is
    /// kept — batch, expiry and serial belong to a lot, not to the drug.
    pub code: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReadScan {
    /// Exactly what the scanner typed, group separators (U+001D) included.
    pub raw: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScanResult {
    /// The product code, as 14 digits; `None` when the scan held none that could be read.
    pub gtin: Option<String>,
    pub display: Option<String>,
    pub batch: Option<String>,
    pub expiry: Option<NaiveDate>,
    /// The drug the code belongs to; `None` for a code no drug has yet.
    pub drug: Option<Drug>,
    /// That drug's original packaging — the one a Wareneingang stocks — once it has one.
    pub original: Option<Packaging>,
}

/// A drug's codes, oldest first.
pub async fn codes_of(pool: &sqlx::PgPool, drug_id: i64) -> AppResult<Vec<DrugCode>> {
    let rows = sqlx::query!(
        "SELECT id, gtin FROM drug_code WHERE drug_id = $1 ORDER BY id",
        drug_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| DrugCode {
            id: row.id,
            display: barcode::display(&row.gtin),
            gtin: row.gtin,
        })
        .collect())
}

#[utoipa::path(
    post,
    path = "/api/drugs/{id}/codes",
    operation_id = "addDrugCode",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = AddDrugCode,
    responses(
        (status = 200, body = Drug),
        (status = 404),
        (status = 409, description = "The code belongs to another drug"),
        (status = 422, description = "No product code could be read")
    )
)]
pub async fn add(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddDrugCode>,
) -> AppResult<Json<Drug>> {
    let gtin = barcode::read(&body.code)
        .gtin
        .ok_or_else(|| AppError::field("code", "code.invalid"))?;
    // The drug first, so a missing one is a 404 rather than a foreign-key 422.
    drugs::load(&state.pool, id).await?;

    let owner: Option<i64> =
        sqlx::query_scalar!("SELECT drug_id FROM drug_code WHERE gtin = $1", gtin)
            .fetch_optional(&state.pool)
            .await?;
    match owner {
        // Scanned twice: already there.
        Some(owner) if owner == id => {}
        Some(_) => {
            return Err(AppError::Conflict(
                "the code belongs to another drug".to_owned(),
            ));
        }
        // A race with another request for the same code ends in the unique index: a 409 too.
        None => {
            sqlx::query!(
                "INSERT INTO drug_code (drug_id, gtin) VALUES ($1, $2)",
                id,
                gtin
            )
            .execute(&state.pool)
            .await?;
        }
    }
    Ok(Json(drugs::load(&state.pool, id).await?))
}

#[utoipa::path(
    delete,
    path = "/api/drug-codes/{id}",
    operation_id = "deleteDrugCode",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 404))
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let deleted = sqlx::query!("DELETE FROM drug_code WHERE id = $1", id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/scan",
    operation_id = "readScan",
    tag = "pharmacy",
    request_body = ReadScan,
    responses((status = 200, body = ScanResult))
)]
pub async fn read_scan(
    State(state): State<AppState>,
    Json(body): Json<ReadScan>,
) -> AppResult<Json<ScanResult>> {
    let scan = barcode::read(&body.raw);
    let drug_id = match &scan.gtin {
        Some(gtin) => {
            sqlx::query_scalar!("SELECT drug_id FROM drug_code WHERE gtin = $1", gtin)
                .fetch_optional(&state.pool)
                .await?
        }
        None => None,
    };
    let (drug, original) = match drug_id {
        Some(drug_id) => {
            let drug = drugs::load(&state.pool, drug_id).await?;
            let mut connection = state.pool.acquire().await?;
            let original = drugs::load_packagings(&mut connection, drug_id, &state.config.pharmacy)
                .await?
                .into_iter()
                .find(|packaging| packaging.kind == PackagingKind::Original);
            (Some(drug), original)
        }
        None => (None, None),
    };
    Ok(Json(ScanResult {
        display: scan.gtin.as_deref().map(barcode::display),
        gtin: scan.gtin,
        batch: scan.batch,
        expiry: scan.expiry,
        drug,
        original,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/drugs/{id}/codes", post(add))
        .route("/drug-codes/{id}", axum::routing::delete(delete))
        .route("/scan", post(read_scan))
}
