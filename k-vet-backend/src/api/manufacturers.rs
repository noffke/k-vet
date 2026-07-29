//! Manufacturers — who produces a drug (T052).
//!
//! Same shape as suppliers; the response type and the shared helpers live in
//! `suppliers.rs`, only the statements differ (sqlx checks each against its table).

use crate::AppState;
use crate::api::common::ListQuery;
use crate::domain::draft::recompute_draft;
use crate::error::{AppError, AppResult};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::api::suppliers::{
    AddressBookEntry, PatchAddressBookEntry, blank_to_null, entry_missing_fields,
};

#[utoipa::path(
    get,
    path = "/api/manufacturers",
    operation_id = "listManufacturers",
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
           FROM manufacturer
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
    path = "/api/manufacturers",
    operation_id = "createManufacturer",
    tag = "pharmacy",
    responses((status = 200, description = "An empty draft", body = AddressBookEntry))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<AddressBookEntry>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO manufacturer DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/manufacturers/{id}",
    operation_id = "getManufacturer",
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
    path = "/api/manufacturers/{id}",
    operation_id = "patchManufacturer",
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
        "SELECT name, draft FROM manufacturer WHERE id = $1 FOR UPDATE",
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
        r#"UPDATE manufacturer SET
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
    path = "/api/manufacturers/{id}/archive",
    operation_id = "archiveManufacturer",
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
    path = "/api/manufacturers/{id}/unarchive",
    operation_id = "unarchiveManufacturer",
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
        "UPDATE manufacturer SET archived = $2 WHERE id = $1",
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
           FROM manufacturer WHERE id = $1"#,
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
        .route("/manufacturers", get(list).post(create))
        .route("/manufacturers/{id}", get(detail).patch(patch))
        .route("/manufacturers/{id}/archive", post(archive))
        .route("/manufacturers/{id}/unarchive", post(unarchive))
}
