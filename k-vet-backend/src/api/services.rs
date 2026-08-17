//! Services: the imported GOT fee schedule and the practice's own positions (T062).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::api::suppliers::blank_to_null;
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::domain::enums::ServiceType;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Service {
    pub id: i64,
    /// `got` (official fee schedule) or `self_defined`.
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    pub name: Option<String>,
    /// For a GOT position its own number; for a self-defined one the position it is charged
    /// analogously to (§ 8 GOT), which is what marks it as such — there is no second flag.
    pub got_number: Option<String>,
    /// Percent; 100 is the single rate.
    pub factor: Option<Decimal>,
    pub vat_percent: Option<Decimal>,
    /// The fee **without** VAT — the GOT publishes net, so that is what is stored and edited.
    pub net_price: Option<Decimal>,
    /// Derived from `net_price` and `vat_percent` so the UI can show what the customer pays.
    pub gross_price: Option<Decimal>,
    /// Travel-expense lines are priced from the kilometres entered (FR-023).
    pub travel_expenses: bool,
    /// Hidden services stay out of pickers and lists by default (FR-021).
    pub hidden: bool,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateService {
    #[serde(rename = "type")]
    pub service_type: ServiceType,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchService {
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub got_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub factor: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub vat_percent: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub net_price: Option<Option<Decimal>>,
    pub travel_expenses: Option<bool>,
    pub hidden: Option<bool>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ServiceFilter {
    /// Include hidden services (the surgical GOT positions, by default).
    pub hidden: Option<bool>,
    #[serde(rename = "type")]
    pub service_type: Option<ServiceType>,
}

/// Row → response, including which mandatory fields are still missing.
macro_rules! service_row {
    ($row:expr) => {{
        let row = $row;
        let mut fields = vec![
            ("name", row.name.is_some()),
            ("vat_percent", row.vat_percent.is_some()),
            ("net_price", row.net_price.is_some()),
        ];
        if row.service_type == ServiceType::Got {
            fields.push(("got_number", row.got_number.is_some()));
            fields.push(("factor", row.factor.is_some()));
        }
        Service {
            missing_fields: missing_fields(&fields)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            id: row.id,
            service_type: row.service_type,
            name: row.name,
            got_number: row.got_number,
            factor: row.factor,
            vat_percent: row.vat_percent,
            net_price: row.net_price,
            gross_price: row
                .net_price
                .zip(row.vat_percent)
                .map(|(net, vat)| crate::domain::money::add_vat(net, vat).gross),
            travel_expenses: row.travel_expenses,
            hidden: row.hidden,
            archived: row.archived,
            draft: row.draft,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }};
}

#[utoipa::path(
    get,
    path = "/api/services",
    operation_id = "listServices",
    tag = "services",
    params(ListQuery, ServiceFilter),
    responses((status = 200, body = Vec<Service>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    Query(filter): Query<ServiceFilter>,
) -> AppResult<Json<Vec<Service>>> {
    let rows = sqlx::query!(
        r#"SELECT id, type AS "service_type: ServiceType", name, got_number, factor,
                  vat_percent, net_price, travel_expenses, hidden, archived, draft,
                  created_at, updated_at
           FROM service
           WHERE ($2 OR NOT archived)
             AND ($3 OR NOT hidden)
             AND ($4::service_type IS NULL OR type = $4)
             AND ($1::text IS NULL
                  OR name ILIKE '%' || $1 || '%'
                  OR got_number = $1)
           ORDER BY got_number NULLS LAST, name NULLS LAST, id
           LIMIT $5 OFFSET $6"#,
        query.search(),
        query.include_archived(),
        filter.hidden.unwrap_or(false),
        filter.service_type as Option<ServiceType>,
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.into_iter().map(|row| service_row!(row)).collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/services",
    operation_id = "createService",
    tag = "services",
    request_body = CreateService,
    responses((status = 200, description = "An empty draft", body = Service))
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateService>,
) -> AppResult<Json<Service>> {
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO service (type) VALUES ($1) RETURNING id",
        body.service_type as ServiceType,
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/services/{id}",
    operation_id = "getService",
    tag = "services",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Service), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Service>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/services/{id}",
    operation_id = "patchService",
    tag = "services",
    params(("id" = i64, Path,)),
    request_body = PatchService,
    responses((status = 200, body = Service), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchService>,
) -> AppResult<Json<Service>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        r#"SELECT type AS "service_type: ServiceType", name, got_number, factor, vat_percent,
                  net_price, draft
           FROM service WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(Some(factor)) = body.factor
        && factor <= Decimal::ZERO
    {
        return Err(AppError::field("factor", "value.mustBePositive"));
    }

    let text_present = |patched: &Option<Option<String>>, stored: &Option<String>| match patched {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => stored.is_some(),
    };
    let number_present = |patched: &Option<Option<Decimal>>, stored: &Option<Decimal>| match patched
    {
        Some(value) => value.is_some(),
        None => stored.is_some(),
    };

    // A GOT position is incomplete without its number and factor. A self-defined one may name
    // the position it is charged analogously to (§ 8 GOT), but is complete without it.
    let mut completeness = vec![
        ("name", text_present(&body.name, &current.name)),
        (
            "vat_percent",
            number_present(&body.vat_percent, &current.vat_percent),
        ),
        (
            "net_price",
            number_present(&body.net_price, &current.net_price),
        ),
    ];
    if current.service_type == ServiceType::Got {
        completeness.push((
            "got_number",
            text_present(&body.got_number, &current.got_number),
        ));
        completeness.push(("factor", number_present(&body.factor, &current.factor)));
    }
    let draft = recompute_draft(current.draft, &completeness)?;

    sqlx::query!(
        r#"UPDATE service SET
               name            = CASE WHEN $2 THEN $3 ELSE name END,
               got_number      = CASE WHEN $4 THEN $5 ELSE got_number END,
               factor          = CASE WHEN $6 THEN $7 ELSE factor END,
               vat_percent     = CASE WHEN $8 THEN $9 ELSE vat_percent END,
               net_price     = CASE WHEN $10 THEN $11 ELSE net_price END,
               travel_expenses = COALESCE($12, travel_expenses),
               hidden          = COALESCE($13, hidden),
               draft           = $14
           WHERE id = $1"#,
        id,
        body.name.is_some(),
        blank_to_null(&body.name),
        body.got_number.is_some(),
        blank_to_null(&body.got_number),
        body.factor.is_some(),
        body.factor.flatten(),
        body.vat_percent.is_some(),
        body.vat_percent.flatten(),
        body.net_price.is_some(),
        body.net_price.flatten(),
        body.travel_expenses,
        body.hidden,
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/services/{id}/archive",
    operation_id = "archiveService",
    tag = "services",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Service))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Service>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/services/{id}/unarchive",
    operation_id = "unarchiveService",
    tag = "services",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Service))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Service>> {
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<Service>> {
    let updated = sqlx::query!(
        "UPDATE service SET archived = $2 WHERE id = $1",
        id,
        archived
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(load(&state.pool, id).await?))
}

pub async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Service> {
    let row = sqlx::query!(
        r#"SELECT id, type AS "service_type: ServiceType", name, got_number, factor,
                  vat_percent, net_price, travel_expenses, hidden, archived, draft,
                  created_at, updated_at
           FROM service WHERE id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(service_row!(row))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/services", get(list).post(create))
        .route("/services/{id}", get(detail).patch(patch))
        .route("/services/{id}/archive", post(archive))
        .route("/services/{id}/unarchive", post(unarchive))
}
