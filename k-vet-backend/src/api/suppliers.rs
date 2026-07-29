//! Suppliers — where original packagings are bought (T052).
//!
//! Manufacturers have the same shape; see `manufacturers.rs`. The two are separate
//! resources because sqlx checks each statement against the real table at compile time.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::error::{AppError, AppResult};

/// An address-book entry: a name and an optional postal address.
#[derive(Debug, Serialize, ToSchema)]
pub struct AddressBookEntry {
    pub id: i64,
    pub name: Option<String>,
    pub addr_addon: Option<String>,
    pub addr_street: Option<String>,
    pub addr_zip: Option<String>,
    pub addr_city: Option<String>,
    /// Database-generated: the address is complete enough to print.
    pub has_address: bool,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchAddressBookEntry {
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub addr_addon: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub addr_street: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub addr_zip: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub addr_city: Option<Option<String>>,
}

/// An emptied text field means "no value", not an empty string.
pub fn blank_to_null(value: &Option<Option<String>>) -> Option<String> {
    value
        .clone()
        .flatten()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

pub fn entry_missing_fields(name: &Option<String>) -> Vec<String> {
    missing_fields(&[("name", name.is_some())])
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[utoipa::path(
    get,
    path = "/api/suppliers",
    operation_id = "listSuppliers",
    tag = "pharmacy",
    params(ListQuery),
    responses((status = 200, body = Vec<AddressBookEntry>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<AddressBookEntry>>> {
    let rows = sqlx::query!(
        r#"SELECT id, name, addr_addon, addr_street, addr_zip, addr_city, has_address,
                  archived, draft, created_at, updated_at
           FROM supplier
           WHERE ($2 OR NOT archived)
             AND ($1::text IS NULL OR name ILIKE '%' || $1 || '%')
           ORDER BY name NULLS LAST, id
           LIMIT $3 OFFSET $4"#,
        query.search(),
        query.include_archived(),
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| AddressBookEntry {
                missing_fields: entry_missing_fields(&row.name),
                id: row.id,
                name: row.name,
                addr_addon: row.addr_addon,
                addr_street: row.addr_street,
                addr_zip: row.addr_zip,
                addr_city: row.addr_city,
                has_address: row.has_address,
                archived: row.archived,
                draft: row.draft,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/suppliers",
    operation_id = "createSupplier",
    tag = "pharmacy",
    responses((status = 200, description = "An empty draft", body = AddressBookEntry))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<AddressBookEntry>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO supplier DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/suppliers/{id}",
    operation_id = "getSupplier",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = AddressBookEntry), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<AddressBookEntry>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/suppliers/{id}",
    operation_id = "patchSupplier",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    request_body = PatchAddressBookEntry,
    responses((status = 200, body = AddressBookEntry), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchAddressBookEntry>,
) -> AppResult<Json<AddressBookEntry>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT name, draft FROM supplier WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let name_present = match &body.name {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => current.name.is_some(),
    };
    let draft = recompute_draft(current.draft, &[("name", name_present)])?;

    sqlx::query!(
        r#"UPDATE supplier SET
               name        = CASE WHEN $2 THEN $3 ELSE name END,
               addr_addon  = CASE WHEN $4 THEN $5 ELSE addr_addon END,
               addr_street = CASE WHEN $6 THEN $7 ELSE addr_street END,
               addr_zip    = CASE WHEN $8 THEN $9 ELSE addr_zip END,
               addr_city   = CASE WHEN $10 THEN $11 ELSE addr_city END,
               draft       = $12
           WHERE id = $1"#,
        id,
        body.name.is_some(),
        blank_to_null(&body.name),
        body.addr_addon.is_some(),
        blank_to_null(&body.addr_addon),
        body.addr_street.is_some(),
        blank_to_null(&body.addr_street),
        body.addr_zip.is_some(),
        blank_to_null(&body.addr_zip),
        body.addr_city.is_some(),
        blank_to_null(&body.addr_city),
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/suppliers/{id}/archive",
    operation_id = "archiveSupplier",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = AddressBookEntry))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<AddressBookEntry>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/suppliers/{id}/unarchive",
    operation_id = "unarchiveSupplier",
    tag = "pharmacy",
    params(("id" = i64, Path,)),
    responses((status = 200, body = AddressBookEntry))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<AddressBookEntry>> {
    set_archived(&state, id, false).await
}

async fn set_archived(
    state: &AppState,
    id: i64,
    archived: bool,
) -> AppResult<Json<AddressBookEntry>> {
    let updated = sqlx::query!(
        "UPDATE supplier SET archived = $2 WHERE id = $1",
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

pub async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<AddressBookEntry> {
    let row = sqlx::query!(
        r#"SELECT id, name, addr_addon, addr_street, addr_zip, addr_city, has_address,
                  archived, draft, created_at, updated_at
           FROM supplier WHERE id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(AddressBookEntry {
        missing_fields: entry_missing_fields(&row.name),
        id: row.id,
        name: row.name,
        addr_addon: row.addr_addon,
        addr_street: row.addr_street,
        addr_zip: row.addr_zip,
        addr_city: row.addr_city,
        has_address: row.has_address,
        archived: row.archived,
        draft: row.draft,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/suppliers", get(list).post(create))
        .route("/suppliers/{id}", get(detail).patch(patch))
        .route("/suppliers/{id}/archive", post(archive))
        .route("/suppliers/{id}/unarchive", post(unarchive))
}
