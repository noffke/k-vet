//! Treatment lines — the billing positions of a treatment (T032).
//!
//! Every billing-relevant value is **pinned when the line is added**: name, price, VAT,
//! factor and GOT number are copied from the catalog and never re-read, so a later price
//! change cannot alter a documented treatment (data-model.md, FR-028).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{MoveDirection, MoveRequest, double_option};
use crate::config::TravelExpenseConfig;
use crate::domain::enums::TreatmentItemKind;
use crate::domain::money;
use crate::domain::stock::{self, LotAllocation};
use crate::error::{AppError, AppResult, FieldError};

#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentItem {
    pub id: i64,
    pub treatment_id: i64,
    pub position: i32,
    pub kind: TreatmentItemKind,
    pub drug_packaging_id: Option<i64>,
    pub service_id: Option<i64>,
    /// The Patientenbehandlung this line belongs to, or null for a line that covers the
    /// visit rather than one animal — the Wegegeld of a house call for two of them.
    pub patient_treatment_id: Option<i64>,
    /// Copied from the catalog; the vet may override it per line.
    pub name: String,
    pub quantity: Decimal,
    pub unit: Option<String>,
    /// GOT factor in percent (100 = single rate).
    pub factor: Option<Decimal>,
    pub got_number: Option<String>,
    /// Per-unit **net** price, pinned at line entry.
    pub price_net: Decimal,
    /// Derived from `price_net` and `vat_percent` so the UI can show the customer-facing price.
    pub price_gross: Decimal,
    pub vat_percent: Decimal,
    /// Travel-expense lines: the kilometres the price was computed from.
    pub km: Option<Decimal>,
    pub km_multiplier: Option<Decimal>,
    /// `true` when the line's service bills travel expenses — the UI then asks for km.
    pub travel_expenses: bool,
    /// Ad-hoc Umwidmung: this dispense is outside the preparation's approval — an eye
    /// preparation used in an ear. Documentation only; it does not move the price.
    pub redesignation: bool,
    /// `price_net × quantity × factor/100`, rounded to cents — **net**.
    pub line_net: Decimal,
    /// `line_net` plus VAT: what the customer pays for this line.
    pub line_gross: Decimal,
    /// Lots the dispense was booked against (drug lines).
    pub lots: Vec<ItemLot>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ItemLot {
    pub lot_id: i64,
    pub quantity: Decimal,
    pub batch_number: Option<String>,
    pub expiration_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTreatmentItem {
    pub kind: TreatmentItemKind,
    /// Required for `drug_packaging` lines.
    pub drug_packaging_id: Option<i64>,
    /// Required for `service` lines.
    pub service_id: Option<i64>,
    /// Defaults to the treatment's only Patientenbehandlung when there is exactly one.
    pub patient_treatment_id: Option<i64>,
    #[serde(default = "one")]
    pub quantity: Decimal,
    /// Travel-expense lines: kilometres driven (one way).
    pub km: Option<Decimal>,
    /// Multiplier for adverse travel conditions (1–3).
    pub km_multiplier: Option<Decimal>,
    /// Ad-hoc Umwidmung; only meaningful on a drug line.
    #[serde(default)]
    pub redesignation: bool,
}

fn one() -> Decimal {
    Decimal::ONE
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchTreatmentItem {
    pub quantity: Option<Decimal>,
    pub price_net: Option<Decimal>,
    pub name: Option<String>,
    pub factor: Option<Decimal>,
    /// Moves the line to another animal, or out of all of them. It lands at the end of the
    /// target's order; its dispensed lots follow, because they hang off the line.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<i64>)]
    pub patient_treatment_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub km: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub km_multiplier: Option<Option<Decimal>>,
    pub redesignation: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LotSelection {
    pub lot_id: i64,
    pub quantity: Decimal,
}

/// Catalog values copied onto a new line.
pub struct CatalogLine {
    pub name: String,
    pub price_net: Decimal,
    pub vat_percent: Decimal,
    pub unit: Option<String>,
    pub factor: Option<Decimal>,
    pub got_number: Option<String>,
    pub travel_expenses: bool,
}

/// Reads the current catalog values for a packaging or service.
///
/// Used when a line is added, when a template is applied and when a duplicate refreshes
/// its prices — the three places where pinning happens.
pub async fn catalog_line(
    connection: &mut PgConnection,
    kind: TreatmentItemKind,
    reference_id: i64,
) -> AppResult<CatalogLine> {
    match kind {
        TreatmentItemKind::DrugPackaging => {
            let row = sqlx::query!(
                r#"SELECT drug.name, drug.vat_percent, packaging.unit, packaging.sales_price_net,
                          packaging.quantity, packaging.draft, packaging.archived
                   FROM drug_packaging packaging
                   JOIN drug ON drug.id = packaging.drug_id
                   WHERE packaging.id = $1"#,
                reference_id,
            )
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| AppError::field("drug_packaging_id", "record.notFound"))?;

            if row.draft {
                return Err(AppError::field("drug_packaging_id", "record.incomplete"));
            }
            let (Some(price), Some(vat)) = (row.sales_price_net, row.vat_percent) else {
                return Err(AppError::field("drug_packaging_id", "record.incomplete"));
            };
            // The line has to say *what* was dispensed: a 10 ml subset and the 100 ml
            // bottle are both "Amoxicillin 100" otherwise (invoice and stock history).
            let drug_name = row.name.unwrap_or_default();
            let name = match (row.quantity, &row.unit) {
                (Some(quantity), Some(unit)) => {
                    format!("{drug_name} · {} {unit}", crate::pdf::number_de(quantity))
                }
                _ => drug_name,
            };
            Ok(CatalogLine {
                name,
                price_net: price,
                vat_percent: vat,
                unit: row.unit,
                factor: None,
                got_number: None,
                travel_expenses: false,
            })
        }
        TreatmentItemKind::Service => {
            let row = sqlx::query!(
                r#"SELECT name, net_price, vat_percent, factor, got_number, travel_expenses,
                          draft, archived
                   FROM service WHERE id = $1"#,
                reference_id,
            )
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| AppError::field("service_id", "record.notFound"))?;

            if row.draft {
                return Err(AppError::field("service_id", "record.incomplete"));
            }
            let (Some(price), Some(vat)) = (row.net_price, row.vat_percent) else {
                return Err(AppError::field("service_id", "record.incomplete"));
            };
            Ok(CatalogLine {
                name: row.name.unwrap_or_default(),
                price_net: price,
                vat_percent: vat,
                unit: None,
                factor: row.factor,
                got_number: row.got_number,
                travel_expenses: row.travel_expenses,
            })
        }
    }
}

#[utoipa::path(
    get,
    operation_id = "listTreatmentItems",
    path = "/api/treatments/{id}/items",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<TreatmentItem>))
)]
pub async fn list(
    State(state): State<AppState>,
    Path(treatment_id): Path<i64>,
) -> AppResult<Json<Vec<TreatmentItem>>> {
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_items(&mut connection, treatment_id).await?))
}

#[utoipa::path(
    post,
    operation_id = "createTreatmentItem",
    path = "/api/treatments/{id}/items",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = CreateTreatmentItem,
    responses((status = 200, body = TreatmentItem), (status = 409), (status = 422))
)]
pub async fn create(
    State(state): State<AppState>,
    Path(treatment_id): Path<i64>,
    Json(body): Json<CreateTreatmentItem>,
) -> AppResult<Json<TreatmentItem>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, treatment_id).await?;

    let reference_id = match body.kind {
        TreatmentItemKind::DrugPackaging => body
            .drug_packaging_id
            .ok_or_else(|| AppError::field("drug_packaging_id", "field.required"))?,
        TreatmentItemKind::Service => body
            .service_id
            .ok_or_else(|| AppError::field("service_id", "field.required"))?,
    };

    let item_id = insert_pinned_item(
        &mut transaction,
        treatment_id,
        body.kind,
        reference_id,
        body.quantity,
        body.patient_treatment_id,
        body.km,
        body.km_multiplier,
        body.redesignation,
        None,
        &state.config.travel_expenses,
    )
    .await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    load_item(&mut connection, item_id).await.map(Json)
}

/// Inserts a line with catalog values pinned; shared by add, apply-template and duplicate.
///
/// `pinned` short-circuits the catalog read when a duplicate copies values verbatim.
// A billing line has this many independent inputs; grouping them into a struct would only
// move the argument list one level out.
#[allow(clippy::too_many_arguments)]
pub async fn insert_pinned_item(
    transaction: &mut Transaction<'_, Postgres>,
    treatment_id: i64,
    kind: TreatmentItemKind,
    reference_id: i64,
    quantity: Decimal,
    patient_treatment_id: Option<i64>,
    km: Option<Decimal>,
    km_multiplier: Option<Decimal>,
    redesignation: bool,
    pinned: Option<CatalogLine>,
    travel: &TravelExpenseConfig,
) -> AppResult<i64> {
    if quantity <= Decimal::ZERO {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }

    let catalog = match pinned {
        Some(values) => values,
        None => catalog_line(transaction, kind, reference_id).await?,
    };

    // A treatment with exactly one animal preselects it (FR-028); drug lines require one,
    // because that is what ties a dispensed batch to the animal that received it.
    let patient_treatment_id = match patient_treatment_id {
        Some(id) => Some(validate_record(transaction, treatment_id, id).await?),
        None => sole_record(transaction, treatment_id).await?,
    };
    if kind == TreatmentItemKind::DrugPackaging && patient_treatment_id.is_none() {
        return Err(AppError::field(
            "patient_treatment_id",
            "item.patientRequired",
        ));
    }

    // A travel-expense line is priced from the distance per GOT § 10; every other line
    // pins the catalog price (FR-023).
    let price_net = match (catalog.travel_expenses, km) {
        (true, Some(km)) => {
            money::travel_expense(km, km_multiplier, travel.rate_per_double_km, travel.minimum)
        }
        _ => catalog.price_net,
    };

    let position = next_position(transaction, treatment_id, patient_treatment_id).await?;

    let (packaging_id, service_id) = match kind {
        TreatmentItemKind::DrugPackaging => (Some(reference_id), None),
        TreatmentItemKind::Service => (None, Some(reference_id)),
    };

    let item_id: i64 = sqlx::query_scalar!(
        r#"INSERT INTO treatment_item
               (treatment_id, position, kind, drug_packaging_id, service_id, patient_treatment_id,
                name, quantity, unit, factor, got_number, price_net, vat_percent,
                km, km_multiplier, redesignation)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
           RETURNING id"#,
        treatment_id,
        position,
        kind as TreatmentItemKind,
        packaging_id,
        service_id,
        patient_treatment_id,
        catalog.name,
        quantity,
        catalog.unit,
        catalog.factor,
        catalog.got_number,
        price_net,
        catalog.vat_percent,
        km,
        km_multiplier,
        redesignation,
    )
    .fetch_one(&mut **transaction)
    .await?;

    if let Some(packaging_id) = packaging_id {
        // Drug lines write draft dispense movements right away (FEFO).
        stock::rewrite_draft_dispenses(transaction, item_id, packaging_id, quantity).await?;
    }
    Ok(item_id)
}

#[utoipa::path(
    patch,
    operation_id = "patchTreatmentItem",
    path = "/api/treatment-items/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = PatchTreatmentItem,
    responses((status = 200, body = TreatmentItem), (status = 409), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchTreatmentItem>,
) -> AppResult<Json<TreatmentItem>> {
    let mut transaction = state.pool.begin().await?;

    // Postgres refuses FOR UPDATE across the nullable side of an outer join, so the
    // travel flag is read separately.
    let current = sqlx::query!(
        r#"SELECT treatment_id, kind AS "kind: TreatmentItemKind", drug_packaging_id, quantity,
                  patient_treatment_id, position, km, km_multiplier, service_id
           FROM treatment_item WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    stock::ensure_editable(&mut transaction, current.treatment_id).await?;

    if let Some(quantity) = body.quantity
        && quantity <= Decimal::ZERO
    {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }
    if let Some(Some(record_id)) = body.patient_treatment_id {
        validate_record(&mut transaction, current.treatment_id, record_id).await?;
    }
    if current.kind == TreatmentItemKind::DrugPackaging && body.patient_treatment_id == Some(None) {
        return Err(AppError::field(
            "patient_treatment_id",
            "item.patientRequired",
        ));
    }

    // Moving a line to another animal takes it out of one order and puts it at the end of
    // the other's, so the positions on both sides stay 1..n.
    let moved_to = match body.patient_treatment_id {
        Some(target) if target != current.patient_treatment_id => {
            close_position_gap(
                &mut transaction,
                current.treatment_id,
                current.patient_treatment_id,
                current.position,
            )
            .await?;
            Some(next_position(&mut transaction, current.treatment_id, target).await?)
        }
        _ => None,
    };

    let travel_service = match current.service_id {
        Some(service_id) => sqlx::query_scalar!(
            "SELECT travel_expenses FROM service WHERE id = $1",
            service_id
        )
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or(false),
        None => false,
    };

    // Re-price a travel line whenever its distance or multiplier changes.
    let travel_price = if travel_service {
        let km = body.km.map_or(current.km, |value| value);
        let multiplier = body
            .km_multiplier
            .map_or(current.km_multiplier, |value| value);
        km.map(|km| {
            money::travel_expense(
                km,
                multiplier,
                state.config.travel_expenses.rate_per_double_km,
                state.config.travel_expenses.minimum,
            )
        })
    } else {
        None
    };
    // An explicit price still wins over the computed one.
    let price_net = body.price_net.or(travel_price);

    sqlx::query!(
        r#"UPDATE treatment_item SET
               quantity      = COALESCE($2, quantity),
               price_net   = COALESCE($3, price_net),
               name          = COALESCE($4, name),
               factor        = CASE WHEN $5 THEN $6 ELSE factor END,
               patient_treatment_id = CASE WHEN $7 THEN $8 ELSE patient_treatment_id END,
               position      = COALESCE($14, position),
               km            = CASE WHEN $9 THEN $10 ELSE km END,
               km_multiplier = CASE WHEN $11 THEN $12 ELSE km_multiplier END,
               redesignation = COALESCE($13, redesignation)
           WHERE id = $1"#,
        id,
        body.quantity,
        price_net,
        body.name,
        body.factor.is_some(),
        body.factor,
        body.patient_treatment_id.is_some(),
        body.patient_treatment_id.flatten(),
        body.km.is_some(),
        body.km.flatten(),
        body.km_multiplier.is_some(),
        body.km_multiplier.flatten(),
        body.redesignation,
        moved_to,
    )
    .execute(&mut *transaction)
    .await?;

    // A changed quantity re-derives the draft dispense allocation.
    if let (Some(quantity), Some(packaging_id)) = (body.quantity, current.drug_packaging_id)
        && quantity != current.quantity
    {
        stock::rewrite_draft_dispenses(&mut transaction, id, packaging_id, quantity).await?;
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    load_item(&mut connection, id).await.map(Json)
}

#[utoipa::path(
    delete,
    operation_id = "deleteTreatmentItem",
    path = "/api/treatment-items/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 409))
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT treatment_id, patient_treatment_id, position FROM treatment_item WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    stock::ensure_editable(&mut transaction, current.treatment_id).await?;

    // Draft movements go with the line (the FK cascades, this is explicit for clarity).
    stock::remove_draft_dispenses(&mut transaction, id).await?;
    sqlx::query!("DELETE FROM treatment_item WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;
    // Close the gap so positions stay 1..n.
    sqlx::query!(
        "UPDATE treatment_item SET position = position - 1
         WHERE treatment_id = $1 AND position > $2",
        current.treatment_id,
        current.position,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    operation_id = "moveTreatmentItem",
    path = "/api/treatment-items/{id}/move",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = MoveRequest,
    responses((status = 200, body = Vec<TreatmentItem>))
)]
pub async fn move_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<MoveRequest>,
) -> AppResult<Json<Vec<TreatmentItem>>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT treatment_id, patient_treatment_id, position FROM treatment_item WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    stock::ensure_editable(&mut transaction, current.treatment_id).await?;

    reorder(
        &mut transaction,
        current.treatment_id,
        current.patient_treatment_id,
        id,
        current.position,
        body.direction,
    )
    .await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        load_items(&mut connection, current.treatment_id).await?,
    ))
}

/// Moves a line one step up or down, or to the top or bottom (FR-029).
///
/// Within its own animal: the treatment page shows one group per animal, and the top of a
/// group is not the top of the invoice. The `UNIQUE (treatment_id, patient_treatment_id,
/// position)` constraint is `DEFERRABLE INITIALLY DEFERRED`, so positions may collide inside
/// the transaction and are checked once at commit.
async fn reorder(
    transaction: &mut Transaction<'_, Postgres>,
    treatment_id: i64,
    patient_treatment_id: Option<i64>,
    id: i64,
    position: i32,
    direction: MoveDirection,
) -> AppResult<()> {
    let max_position: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) FROM treatment_item
          WHERE treatment_id = $1 AND patient_treatment_id IS NOT DISTINCT FROM $2",
        treatment_id,
        patient_treatment_id,
    )
    .fetch_one(&mut **transaction)
    .await?
    .unwrap_or(0);

    match direction {
        MoveDirection::Up | MoveDirection::Down => {
            let target = if direction == MoveDirection::Up {
                position - 1
            } else {
                position + 1
            };
            if target < 1 || target > max_position {
                return Ok(());
            }
            // Swap with the neighbour.
            sqlx::query!(
                "UPDATE treatment_item SET position = $1
                 WHERE treatment_id = $2 AND patient_treatment_id IS NOT DISTINCT FROM $3
                   AND position = $4",
                position,
                treatment_id,
                patient_treatment_id,
                target,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!(
                "UPDATE treatment_item SET position = $1 WHERE id = $2",
                target,
                id
            )
            .execute(&mut **transaction)
            .await?;
        }
        MoveDirection::Top => {
            if position == 1 {
                return Ok(());
            }
            sqlx::query!(
                "UPDATE treatment_item SET position = position + 1
                 WHERE treatment_id = $1 AND patient_treatment_id IS NOT DISTINCT FROM $2
                   AND position < $3",
                treatment_id,
                patient_treatment_id,
                position,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!("UPDATE treatment_item SET position = 1 WHERE id = $1", id)
                .execute(&mut **transaction)
                .await?;
        }
        MoveDirection::Bottom => {
            if position == max_position {
                return Ok(());
            }
            sqlx::query!(
                "UPDATE treatment_item SET position = position - 1
                 WHERE treatment_id = $1 AND patient_treatment_id IS NOT DISTINCT FROM $2
                   AND position > $3",
                treatment_id,
                patient_treatment_id,
                position,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!(
                "UPDATE treatment_item SET position = $1 WHERE id = $2",
                max_position,
                id
            )
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

#[utoipa::path(
    post,
    operation_id = "setTreatmentItemLots",
    path = "/api/treatment-items/{id}/lots",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = Vec<LotSelection>,
    responses((status = 200, body = TreatmentItem), (status = 409), (status = 422))
)]
pub async fn set_lots(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(selections): Json<Vec<LotSelection>>,
) -> AppResult<Json<TreatmentItem>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT treatment_id, drug_packaging_id, quantity FROM treatment_item WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    stock::ensure_editable(&mut transaction, current.treatment_id).await?;

    let packaging_id = current
        .drug_packaging_id
        .ok_or_else(|| AppError::BadRequest("only drug lines have lots".to_owned()))?;
    let target = stock::stock_target(&mut *transaction, packaging_id)
        .await?
        .ok_or_else(|| AppError::field("drug_packaging_id", "lot.noOriginalPackaging"))?;

    // Lot quantities are base units (ml, pieces); the line counts packagings.
    let needed = current.quantity * target.base_units;
    let selected: Decimal = selections.iter().map(|selection| selection.quantity).sum();
    if selected != needed {
        return Err(AppError::Validation(vec![FieldError::new(
            "quantity",
            "item.lotQuantityMismatch",
        )]));
    }
    // Every chosen lot must belong to the drug's original packaging.
    for selection in &selections {
        let belongs: Option<bool> = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM drug_stock_lot WHERE id = $1 AND packaging_id = $2)",
            selection.lot_id,
            target.packaging_id,
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !belongs.unwrap_or(false) {
            return Err(AppError::field("lot_id", "lot.notForThisPackaging"));
        }
    }

    let allocations: Vec<LotAllocation> = selections
        .into_iter()
        .map(|selection| LotAllocation {
            lot_id: selection.lot_id,
            quantity: selection.quantity,
        })
        .collect();
    stock::rewrite_draft_dispenses_with_lots(&mut transaction, id, &allocations).await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    load_item(&mut connection, id).await.map(Json)
}

/// Rejects a patient that does not belong to the treatment.
async fn validate_record(
    connection: &mut PgConnection,
    treatment_id: i64,
    patient_treatment_id: i64,
) -> AppResult<i64> {
    let exists: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM patient_treatment WHERE treatment_id = $1 AND id = $2
         )",
        treatment_id,
        patient_treatment_id,
    )
    .fetch_one(&mut *connection)
    .await?;
    if exists.unwrap_or(false) {
        Ok(patient_treatment_id)
    } else {
        Err(AppError::field(
            "patient_treatment_id",
            "item.patientNotInTreatment",
        ))
    }
}

/// The treatment's only animal record, when it has exactly one — what a new line defaults to.
async fn sole_record(connection: &mut PgConnection, treatment_id: i64) -> AppResult<Option<i64>> {
    let records = sqlx::query_scalar!(
        "SELECT id FROM patient_treatment WHERE treatment_id = $1",
        treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(match records.as_slice() {
        [only] => Some(*only),
        _ => None,
    })
}

/// The next free position within one owner — an animal's record, or the treatment itself.
async fn next_position(
    connection: &mut PgConnection,
    treatment_id: i64,
    patient_treatment_id: Option<i64>,
) -> AppResult<i32> {
    Ok(sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM treatment_item
          WHERE treatment_id = $1 AND patient_treatment_id IS NOT DISTINCT FROM $2",
        treatment_id,
        patient_treatment_id,
    )
    .fetch_one(&mut *connection)
    .await?
    .unwrap_or(1))
}

/// Closes the hole a line leaves when it moves to another owner or is deleted.
async fn close_position_gap(
    connection: &mut PgConnection,
    treatment_id: i64,
    patient_treatment_id: Option<i64>,
    position: i32,
) -> AppResult<()> {
    sqlx::query!(
        "UPDATE treatment_item SET position = position - 1
          WHERE treatment_id = $1 AND patient_treatment_id IS NOT DISTINCT FROM $2
            AND position > $3",
        treatment_id,
        patient_treatment_id,
        position,
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn load_item(connection: &mut PgConnection, id: i64) -> AppResult<TreatmentItem> {
    let treatment_id: i64 =
        sqlx::query_scalar!("SELECT treatment_id FROM treatment_item WHERE id = $1", id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(AppError::NotFound)?;
    let items = load_items(connection, treatment_id).await?;
    items
        .into_iter()
        .find(|item| item.id == id)
        .ok_or(AppError::NotFound)
}

/// Loads a treatment's lines in position order, each with its booked lots.
pub async fn load_items(
    connection: &mut PgConnection,
    treatment_id: i64,
) -> AppResult<Vec<TreatmentItem>> {
    let rows = sqlx::query!(
        r#"SELECT item.id, item.treatment_id, item.position,
                  item.kind AS "kind: TreatmentItemKind", item.drug_packaging_id,
                  item.service_id, item.patient_treatment_id, item.name, item.quantity, item.unit,
                  item.factor, item.got_number, item.price_net, item.vat_percent, item.km,
                  item.redesignation,
                  item.km_multiplier, item.created_at,
                  COALESCE(service.travel_expenses, false) AS "travel_expenses!"
           FROM treatment_item item
           LEFT JOIN service ON service.id = item.service_id
           WHERE item.treatment_id = $1 ORDER BY item.position"#,
        treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    // Only the dispenses that are still in effect: a reversed one belongs to a cancelled
    // invoice and is history, not part of this line's current allocation.
    let lots = sqlx::query!(
        r#"SELECT movement.treatment_item_id, movement.lot_id, movement.quantity,
                  lot.batch_number, lot.expiration_date
           FROM drug_stock_movement movement
           JOIN drug_stock_lot lot ON lot.id = movement.lot_id
           JOIN treatment_item item ON item.id = movement.treatment_item_id
           WHERE item.treatment_id = $1
             AND movement.kind = 'dispense'
             AND NOT EXISTS (
                 SELECT 1 FROM drug_stock_movement reversal
                 WHERE reversal.reverses_movement_id = movement.id
             )
           ORDER BY movement.id"#,
        treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TreatmentItem {
            line_net: money::line_total(row.price_net, row.quantity, row.factor),
            line_gross: money::add_vat(
                money::line_total(row.price_net, row.quantity, row.factor),
                row.vat_percent,
            )
            .gross,
            price_gross: money::add_vat(row.price_net, row.vat_percent).gross,
            redesignation: row.redesignation,
            lots: lots
                .iter()
                .filter(|lot| lot.treatment_item_id == Some(row.id))
                .map(|lot| ItemLot {
                    lot_id: lot.lot_id,
                    // Dispenses are stored negative; the UI shows the dispensed amount.
                    quantity: -lot.quantity,
                    batch_number: lot.batch_number.clone(),
                    expiration_date: lot.expiration_date,
                })
                .collect(),
            id: row.id,
            treatment_id: row.treatment_id,
            position: row.position,
            kind: row.kind,
            drug_packaging_id: row.drug_packaging_id,
            service_id: row.service_id,
            patient_treatment_id: row.patient_treatment_id,
            name: row.name,
            quantity: row.quantity,
            unit: row.unit,
            factor: row.factor,
            got_number: row.got_number,
            price_net: row.price_net,
            vat_percent: row.vat_percent,
            km: row.km,
            km_multiplier: row.km_multiplier,
            travel_expenses: row.travel_expenses,
            created_at: row.created_at,
        })
        .collect())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/treatment-items/{id}",
            axum::routing::patch(patch).delete(delete),
        )
        .route("/treatment-items/{id}/move", post(move_item))
        .route("/treatment-items/{id}/lots", post(set_lots))
}
