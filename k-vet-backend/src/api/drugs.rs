//! Drugs and their packagings (T053).
//!
//! The sales price of a packaging is computed per AMPreisV whenever the purchase price,
//! the quantity or the VAT rate changes — unless the vet overrode it by hand (FR-014).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::config::PharmacyConfig;
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::domain::enums::PackagingKind;
use crate::domain::money;
use crate::error::{AppError, AppResult};

/// Turns the drug's flag and the practice's configuration into the pricing policy.
///
/// Kept in one place so every price — stored, recomputed and previewed — comes from the same
/// decision rather than three near-copies of it.
pub fn pricing_policy(human_drug: bool, pharmacy: &PharmacyConfig) -> money::PricingPolicy {
    money::PricingPolicy {
        rule: if human_drug {
            money::DrugRule::Human
        } else {
            money::DrugRule::Veterinary
        },
        subset_proportional_floor: pharmacy.subset_never_below_proportional,
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Drug {
    pub id: i64,
    pub name: Option<String>,
    pub manufacturer_id: Option<i64>,
    pub manufacturer_name: Option<String>,
    /// Informational flags in this version — no reminders or ledgers (FR-012).
    pub submission_receipt: bool,
    pub narcotic: bool,
    pub vaccine: bool,
    pub refrigerate: bool,
    pub redesignation: bool,
    /// A medicine approved for humans, dispensed for use in an animal: priced by
    /// § 3 Abs. 1 Satz 2 AMPreisV rather than by the veterinary bands.
    pub human_drug: bool,
    pub vat_percent: Option<Decimal>,
    pub approval_number: Option<String>,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    /// Derived stock over the lots of the original packaging, in base units.
    pub in_stock: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Packaging {
    pub id: i64,
    pub drug_id: i64,
    pub kind: PackagingKind,
    pub unit: Option<String>,
    pub quantity: Option<Decimal>,
    /// Purchase price without VAT; derived pro rata for subsets.
    pub list_price_net: Option<Decimal>,
    /// Sales price **without** VAT — the figure AMPreisV computes and the vet edits.
    pub sales_price_net: Option<Decimal>,
    /// Derived from `sales_price_net`; what the customer pays.
    pub sales_price_gross: Option<Decimal>,
    /// `true` while the vet's manual price wins over the computed one.
    pub price_overridden: bool,
    /// What AMPreisV would charge, net — shown next to an overridden price.
    pub computed_price_net: Option<Decimal>,
    pub supplier_id: Option<i64>,
    pub supplier_name: Option<String>,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchDrug {
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub manufacturer_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub vat_percent: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub approval_number: Option<Option<String>>,
    pub submission_receipt: Option<bool>,
    pub narcotic: Option<bool>,
    pub vaccine: Option<bool>,
    pub refrigerate: Option<bool>,
    pub redesignation: Option<bool>,
    pub human_drug: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePackaging {
    /// `original` (bought and stocked) or `subset` (dispensed from the original).
    pub kind: PackagingKind,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchPackaging {
    #[serde(default, deserialize_with = "double_option")]
    pub unit: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub quantity: Option<Option<Decimal>>,
    /// Only meaningful for originals; subsets derive it pro rata.
    #[serde(default, deserialize_with = "double_option")]
    pub list_price_net: Option<Option<Decimal>>,
    /// Setting this overrides the computed price; `null` returns to the computed one.
    #[serde(default, deserialize_with = "double_option")]
    pub sales_price_net: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub supplier_id: Option<Option<i64>>,
}

#[utoipa::path(
    get,
    path = "/api/drugs",
    operation_id = "listDrugs",
    tag = "pharmacy",
    params(ListQuery),
    responses((status = 200, body = Vec<Drug>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<Drug>>> {
    let rows = sqlx::query!(
        r#"SELECT drug.id
           FROM drug
           LEFT JOIN manufacturer ON manufacturer.id = drug.manufacturer_id
           WHERE ($2 OR NOT drug.archived)
             AND ($1::text IS NULL
                  OR drug.name ILIKE '%' || $1 || '%'
                  OR drug.approval_number ILIKE '%' || $1 || '%'
                  OR manufacturer.name ILIKE '%' || $1 || '%')
           ORDER BY drug.name NULLS LAST, drug.id
           LIMIT $3 OFFSET $4"#,
        query.search(),
        query.include_archived(),
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    let mut drugs = Vec::with_capacity(rows.len());
    for row in rows {
        drugs.push(load(&state.pool, row.id).await?);
    }
    Ok(Json(drugs))
}

#[utoipa::path(
    post,
    path = "/api/drugs",
    operation_id = "createDrug",
    tag = "pharmacy",
    responses((status = 200, description = "An empty draft", body = Drug))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<Drug>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO drug DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/drugs/{id}",
    operation_id = "getDrug",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Drug), (status = 404))
)]
pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Drug>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/drugs/{id}",
    operation_id = "patchDrug",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = PatchDrug,
    responses((status = 200, body = Drug), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchDrug>,
) -> AppResult<Json<Drug>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT name, manufacturer_id, vat_percent, draft FROM drug WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(Some(vat)) = body.vat_percent
        && vat.is_sign_negative()
    {
        return Err(AppError::field("vat_percent", "value.mustBePositive"));
    }

    let name_present = match &body.name {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => current.name.is_some(),
    };
    let manufacturer_present = match &body.manufacturer_id {
        Some(value) => value.is_some(),
        None => current.manufacturer_id.is_some(),
    };
    let vat_present = match &body.vat_percent {
        Some(value) => value.is_some(),
        None => current.vat_percent.is_some(),
    };
    let draft = recompute_draft(
        current.draft,
        &[
            ("name", name_present),
            ("manufacturer_id", manufacturer_present),
            ("vat_percent", vat_present),
        ],
    )?;

    sqlx::query!(
        r#"UPDATE drug SET
               name               = CASE WHEN $2 THEN $3 ELSE name END,
               manufacturer_id    = CASE WHEN $4 THEN $5 ELSE manufacturer_id END,
               vat_percent        = CASE WHEN $6 THEN $7 ELSE vat_percent END,
               approval_number    = CASE WHEN $8 THEN $9 ELSE approval_number END,
               submission_receipt = COALESCE($10, submission_receipt),
               narcotic           = COALESCE($11, narcotic),
               vaccine            = COALESCE($12, vaccine),
               refrigerate        = COALESCE($13, refrigerate),
               redesignation      = COALESCE($14, redesignation),
               human_drug         = COALESCE($15, human_drug),
               draft              = $16
           WHERE id = $1"#,
        id,
        body.name.is_some(),
        crate::api::suppliers::blank_to_null(&body.name),
        body.manufacturer_id.is_some(),
        body.manufacturer_id.flatten(),
        body.vat_percent.is_some(),
        body.vat_percent.flatten(),
        body.approval_number.is_some(),
        crate::api::suppliers::blank_to_null(&body.approval_number),
        body.submission_receipt,
        body.narcotic,
        body.vaccine,
        body.refrigerate,
        body.redesignation,
        body.human_drug,
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    // A changed VAT rate moves every computed price of this drug.
    recompute_prices(&mut transaction, id, &state.config.pharmacy).await?;
    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/drugs/{id}/archive",
    operation_id = "archiveDrug",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Drug))
)]
pub async fn archive(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Drug>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/drugs/{id}/unarchive",
    operation_id = "unarchiveDrug",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Drug))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Drug>> {
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<Drug>> {
    let updated = sqlx::query!("UPDATE drug SET archived = $2 WHERE id = $1", id, archived)
        .execute(&state.pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/drugs/{id}/packagings",
    operation_id = "listPackagings",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<Packaging>))
)]
pub async fn list_packagings(
    State(state): State<AppState>,
    Path(drug_id): Path<i64>,
) -> AppResult<Json<Vec<Packaging>>> {
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        load_packagings(&mut connection, drug_id, &state.config.pharmacy).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/drugs/{id}/packagings",
    operation_id = "createPackaging",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = CreatePackaging,
    responses(
        (status = 200, description = "An empty draft packaging", body = Packaging),
        (status = 409, description = "The drug already has an original packaging")
    )
)]
pub async fn create_packaging(
    State(state): State<AppState>,
    Path(drug_id): Path<i64>,
    Json(body): Json<CreatePackaging>,
) -> AppResult<Json<Packaging>> {
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO drug_packaging (drug_id, kind) VALUES ($1, $2) RETURNING id",
        drug_id,
        body.kind as PackagingKind,
    )
    .fetch_one(&state.pool)
    .await?;

    let mut connection = state.pool.acquire().await?;
    load_packaging(&mut connection, id, &state.config.pharmacy)
        .await
        .map(Json)
}

#[utoipa::path(
    patch,
    path = "/api/packagings/{id}",
    operation_id = "patchPackaging",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = PatchPackaging,
    responses((status = 200, body = Packaging), (status = 404), (status = 422))
)]
pub async fn patch_packaging(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPackaging>,
) -> AppResult<Json<Packaging>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        r#"SELECT drug_id, kind AS "kind: PackagingKind", unit, quantity, list_price_net,
                  supplier_id, price_overridden, draft
           FROM drug_packaging WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(Some(quantity)) = body.quantity
        && quantity <= Decimal::ZERO
    {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }

    // An explicit price is the vet's override; `null` hands the price back to AMPreisV.
    let price_overridden = match &body.sales_price_net {
        Some(Some(_)) => true,
        Some(None) => false,
        None => current.price_overridden,
    };

    let unit_present = match &body.unit {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => current.unit.is_some(),
    };
    let quantity_present = match &body.quantity {
        Some(value) => value.is_some(),
        None => current.quantity.is_some(),
    };
    // Subsets derive their list price, so it counts as present once the quantity is known.
    let list_price_present = match (&body.list_price_net, current.kind) {
        (Some(value), PackagingKind::Original) => value.is_some(),
        (None, PackagingKind::Original) => current.list_price_net.is_some(),
        (_, PackagingKind::Subset) => quantity_present,
    };
    let supplier_present = match &body.supplier_id {
        Some(value) => value.is_some(),
        None => current.supplier_id.is_some(),
    };

    sqlx::query!(
        r#"UPDATE drug_packaging SET
               unit              = CASE WHEN $2 THEN $3 ELSE unit END,
               quantity          = CASE WHEN $4 THEN $5 ELSE quantity END,
               list_price_net    = CASE WHEN $6 THEN $7 ELSE list_price_net END,
               sales_price_net = CASE WHEN $8 THEN $9 ELSE sales_price_net END,
               supplier_id       = CASE WHEN $10 THEN $11 ELSE supplier_id END,
               price_overridden  = $12
           WHERE id = $1"#,
        id,
        body.unit.is_some(),
        crate::api::suppliers::blank_to_null(&body.unit),
        body.quantity.is_some(),
        body.quantity.flatten(),
        body.list_price_net.is_some() && current.kind == PackagingKind::Original,
        body.list_price_net.flatten(),
        matches!(body.sales_price_net, Some(Some(_))),
        body.sales_price_net.flatten(),
        body.supplier_id.is_some(),
        body.supplier_id.flatten(),
        price_overridden,
    )
    .execute(&mut *transaction)
    .await?;

    // Recompute every derived price of the drug: a changed original moves its subsets.
    recompute_prices(&mut transaction, current.drug_id, &state.config.pharmacy).await?;

    // The completeness flag is decided after the recompute filled the prices in.
    let after = sqlx::query!(
        "SELECT unit, quantity, list_price_net, sales_price_net FROM drug_packaging
         WHERE id = $1",
        id
    )
    .fetch_one(&mut *transaction)
    .await?;
    let draft = recompute_draft(
        current.draft,
        &[
            ("unit", unit_present),
            ("quantity", quantity_present),
            (
                "list_price_net",
                list_price_present && after.list_price_net.is_some(),
            ),
            ("sales_price_net", after.sales_price_net.is_some()),
            // Only original packagings are bought from a supplier.
            (
                "supplier_id",
                current.kind == PackagingKind::Subset || supplier_present,
            ),
        ],
    )?;
    sqlx::query!(
        "UPDATE drug_packaging SET draft = $2 WHERE id = $1",
        id,
        draft
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    load_packaging(&mut connection, id, &state.config.pharmacy)
        .await
        .map(Json)
}

/// Recomputes the AMPreisV price of every packaging of a drug that is not overridden.
///
/// The original's price follows § 3(3)/(4) capped by § 10(2); a subset takes its list
/// price pro rata from the original and adds the § 4 Teilmengenzuschlag.
pub async fn recompute_prices(
    connection: &mut PgConnection,
    drug_id: i64,
    pharmacy: &PharmacyConfig,
) -> AppResult<()> {
    let drug = sqlx::query!(
        "SELECT vat_percent, human_drug FROM drug WHERE id = $1",
        drug_id
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;
    let vat = drug.vat_percent.unwrap_or(Decimal::ZERO);
    let policy = pricing_policy(drug.human_drug, pharmacy);

    let original = sqlx::query!(
        "SELECT id, quantity, list_price_net FROM drug_packaging
         WHERE drug_id = $1 AND kind = 'original'",
        drug_id,
    )
    .fetch_optional(&mut *connection)
    .await?;

    if let Some(original) = &original
        && let Some(list_price) = original.list_price_net
    {
        let price = money::drug_price_original(list_price, vat, policy);
        sqlx::query!(
            "UPDATE drug_packaging SET sales_price_net = $2
             WHERE id = $1 AND NOT price_overridden",
            original.id,
            price.net,
        )
        .execute(&mut *connection)
        .await?;
    }

    let (Some(original), Some(original_quantity), Some(original_price)) = (
        original.as_ref().map(|row| row.id),
        original.as_ref().and_then(|row| row.quantity),
        original.as_ref().and_then(|row| row.list_price_net),
    ) else {
        return Ok(());
    };
    let _ = original;

    let subsets = sqlx::query!(
        "SELECT id, quantity FROM drug_packaging WHERE drug_id = $1 AND kind = 'subset'",
        drug_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    for subset in subsets {
        let Some(quantity) = subset.quantity else {
            continue;
        };
        let price =
            money::drug_price_subset(original_price, original_quantity, quantity, vat, policy);
        let list_price = money::subset_list_price(original_price, original_quantity, quantity);
        sqlx::query!(
            "UPDATE drug_packaging SET
                 list_price_net    = $2,
                 sales_price_net = CASE WHEN price_overridden THEN sales_price_net ELSE $3 END
             WHERE id = $1",
            subset.id,
            list_price,
            price.net,
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub async fn load(pool: &PgPool, id: i64) -> AppResult<Drug> {
    let row = sqlx::query!(
        r#"SELECT drug.id, drug.name, drug.manufacturer_id, drug.submission_receipt,
                  drug.narcotic, drug.vaccine, drug.refrigerate, drug.redesignation,
                  drug.human_drug,
                  drug.vat_percent, drug.approval_number, drug.archived, drug.draft,
                  drug.created_at, drug.updated_at,
                  manufacturer.name AS "manufacturer_name?",
                  (SELECT COALESCE(SUM(remaining), 0) FROM lot_remaining
                    WHERE lot_remaining.drug_id = drug.id) AS "in_stock?"
           FROM drug
           LEFT JOIN manufacturer ON manufacturer.id = drug.manufacturer_id
           WHERE drug.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let missing = missing_fields(&[
        ("name", row.name.is_some()),
        ("manufacturer_id", row.manufacturer_id.is_some()),
        ("vat_percent", row.vat_percent.is_some()),
    ])
    .into_iter()
    .map(str::to_owned)
    .collect();

    Ok(Drug {
        id: row.id,
        name: row.name,
        manufacturer_id: row.manufacturer_id,
        manufacturer_name: row.manufacturer_name,
        submission_receipt: row.submission_receipt,
        narcotic: row.narcotic,
        vaccine: row.vaccine,
        refrigerate: row.refrigerate,
        redesignation: row.redesignation,
        human_drug: row.human_drug,
        vat_percent: row.vat_percent,
        approval_number: row.approval_number,
        archived: row.archived,
        draft: row.draft,
        missing_fields: missing,
        in_stock: row.in_stock.unwrap_or_default(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub async fn load_packaging(
    connection: &mut PgConnection,
    id: i64,
    pharmacy: &PharmacyConfig,
) -> AppResult<Packaging> {
    let drug_id: i64 = sqlx::query_scalar!("SELECT drug_id FROM drug_packaging WHERE id = $1", id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(AppError::NotFound)?;
    load_packagings(connection, drug_id, pharmacy)
        .await?
        .into_iter()
        .find(|packaging| packaging.id == id)
        .ok_or(AppError::NotFound)
}

pub async fn load_packagings(
    connection: &mut PgConnection,
    drug_id: i64,
    pharmacy: &PharmacyConfig,
) -> AppResult<Vec<Packaging>> {
    let rows = sqlx::query!(
        r#"SELECT packaging.id, packaging.drug_id, packaging.kind AS "kind: PackagingKind",
                  packaging.unit, packaging.quantity, packaging.list_price_net,
                  packaging.sales_price_net, packaging.price_overridden,
                  packaging.supplier_id, packaging.archived, packaging.draft,
                  supplier.name AS "supplier_name?", drug.vat_percent, drug.human_drug
           FROM drug_packaging packaging
           JOIN drug ON drug.id = packaging.drug_id
           LEFT JOIN supplier ON supplier.id = packaging.supplier_id
           WHERE packaging.drug_id = $1
           ORDER BY packaging.kind, packaging.quantity NULLS LAST, packaging.id"#,
        drug_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    // The original's values are the basis for every subset's computed price.
    let original = rows
        .iter()
        .find(|row| row.kind == PackagingKind::Original)
        .map(|row| (row.quantity, row.list_price_net));

    Ok(rows
        .iter()
        .map(|row| {
            let vat = row.vat_percent.unwrap_or(Decimal::ZERO);
            let policy = pricing_policy(row.human_drug, pharmacy);
            let computed = match row.kind {
                PackagingKind::Original => row
                    .list_price_net
                    .map(|list_price| money::drug_price_original(list_price, vat, policy).net),
                PackagingKind::Subset => match (original, row.quantity) {
                    (Some((Some(original_quantity), Some(original_price))), Some(quantity)) => {
                        Some(
                            money::drug_price_subset(
                                original_price,
                                original_quantity,
                                quantity,
                                vat,
                                policy,
                            )
                            .net,
                        )
                    }
                    _ => None,
                },
            };
            Packaging {
                missing_fields: missing_fields(&[
                    ("unit", row.unit.is_some()),
                    ("quantity", row.quantity.is_some()),
                    ("list_price_net", row.list_price_net.is_some()),
                    ("sales_price_net", row.sales_price_net.is_some()),
                    (
                        "supplier_id",
                        row.kind == PackagingKind::Subset || row.supplier_id.is_some(),
                    ),
                ])
                .into_iter()
                .map(str::to_owned)
                .collect(),
                id: row.id,
                drug_id: row.drug_id,
                kind: row.kind,
                unit: row.unit.clone(),
                quantity: row.quantity,
                list_price_net: row.list_price_net,
                sales_price_net: row.sales_price_net,
                price_overridden: row.price_overridden,
                computed_price_net: computed,
                sales_price_gross: row
                    .sales_price_net
                    .map(|net| money::add_vat(net, row.vat_percent.unwrap_or(Decimal::ZERO)).gross),
                supplier_id: row.supplier_id,
                supplier_name: row.supplier_name.clone(),
                archived: row.archived,
                draft: row.draft,
            }
        })
        .collect())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/drugs", get(list).post(create))
        .route("/drugs/{id}", get(detail).patch(patch))
        .route("/drugs/{id}/archive", post(archive))
        .route("/drugs/{id}/unarchive", post(unarchive))
        .route(
            "/drugs/{id}/packagings",
            get(list_packagings).post(create_packaging),
        )
        .route("/packagings/{id}", axum::routing::patch(patch_packaging))
}
