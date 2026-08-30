//! Textbausteine — the library of reusable snippets for a Vorbericht or a Therapie.
//!
//! A block is a name and its text, nothing more: where it was used is deliberately not
//! recorded (see migration `0018`). CRUD plus archiving, the same master-data shape as
//! manufacturers and suppliers.

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

#[derive(Debug, Serialize, ToSchema)]
pub struct TextBlock {
    pub id: i64,
    pub name: Option<String>,
    pub content: Option<String>,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchTextBlock {
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub content: Option<Option<String>>,
}

/// Trims to `None`, so a field the vet blanked out is absent rather than an empty string.
fn blank_to_null(value: &Option<Option<String>>) -> Option<String> {
    value
        .as_ref()
        .and_then(|inner| inner.as_ref())
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn block_missing_fields(name: &Option<String>, content: &Option<String>) -> Vec<String> {
    missing_fields(&[("name", name.is_some()), ("content", content.is_some())])
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[utoipa::path(
    get,
    path = "/api/text-blocks",
    operation_id = "listTextBlocks",
    tag = "textBlocks",
    params(ListQuery),
    responses((status = 200, body = Vec<TextBlock>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<TextBlock>>> {
    // Content is searched alongside the name: the vet remembers the wording of a block long
    // before they remember what they called it.
    let rows = sqlx::query!(
        r#"SELECT id, name, content, archived, draft, created_at, updated_at
           FROM text_block
           WHERE ($2 OR NOT archived)
             AND ($1::text IS NULL
                  OR name ILIKE '%' || $1 || '%'
                  OR content ILIKE '%' || $1 || '%')
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
            .map(|row| TextBlock {
                missing_fields: block_missing_fields(&row.name, &row.content),
                id: row.id,
                name: row.name,
                content: row.content,
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
    path = "/api/text-blocks",
    operation_id = "createTextBlock",
    tag = "textBlocks",
    responses((status = 200, description = "An empty draft, ready for auto-save", body = TextBlock))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<TextBlock>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO text_block DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/text-blocks/{id}",
    operation_id = "getTextBlock",
    tag = "textBlocks",
    params(("id" = i64, Path,)),
    responses((status = 200, body = TextBlock), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<TextBlock>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/text-blocks/{id}",
    operation_id = "patchTextBlock",
    tag = "textBlocks",
    params(("id" = i64, Path,)),
    request_body = PatchTextBlock,
    responses((status = 200, body = TextBlock), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchTextBlock>,
) -> AppResult<Json<TextBlock>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT name, content, draft FROM text_block WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let present = |patched: &Option<Option<String>>, stored: &Option<String>| match patched {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => stored.is_some(),
    };
    let draft = recompute_draft(
        current.draft,
        &[
            ("name", present(&body.name, &current.name)),
            ("content", present(&body.content, &current.content)),
        ],
    )?;

    sqlx::query!(
        r#"UPDATE text_block SET
               name    = CASE WHEN $2 THEN $3 ELSE name END,
               content = CASE WHEN $4 THEN $5 ELSE content END,
               draft   = $6
           WHERE id = $1"#,
        id,
        body.name.is_some(),
        blank_to_null(&body.name),
        body.content.is_some(),
        blank_to_null(&body.content),
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/text-blocks/{id}/archive",
    operation_id = "archiveTextBlock",
    tag = "textBlocks",
    params(("id" = i64, Path,)),
    responses((status = 200, body = TextBlock))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<TextBlock>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/text-blocks/{id}/unarchive",
    operation_id = "unarchiveTextBlock",
    tag = "textBlocks",
    params(("id" = i64, Path,)),
    responses((status = 200, body = TextBlock))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<TextBlock>> {
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<TextBlock>> {
    let updated = sqlx::query!(
        "UPDATE text_block SET archived = $2 WHERE id = $1",
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

async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<TextBlock> {
    let row = sqlx::query!(
        r#"SELECT id, name, content, archived, draft, created_at, updated_at
           FROM text_block WHERE id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(TextBlock {
        missing_fields: block_missing_fields(&row.name, &row.content),
        id: row.id,
        name: row.name,
        content: row.content,
        archived: row.archived,
        draft: row.draft,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/text-blocks", get(list).post(create))
        .route("/text-blocks/{id}", get(detail).patch(patch))
        .route("/text-blocks/{id}/archive", post(archive))
        .route("/text-blocks/{id}/unarchive", post(unarchive))
}
