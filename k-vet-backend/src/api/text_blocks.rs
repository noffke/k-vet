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
use sqlx::types::Json as Jsonb;
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::domain::rich_text::{self, RichText};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct TextBlock {
    pub id: i64,
    pub name: Option<String>,
    /// The block's formatted text, inserted as it is; `null` while none is written.
    pub content: Option<RichText>,
    /// The same text plain — a line per paragraph or list item, formatting dropped — for the
    /// previews in the list and the picker. Derived by the database (migration `0030`).
    pub content_text: Option<String>,
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
    /// A document without text counts as no content: the block stays a draft.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<RichText>)]
    pub content: Option<Option<RichText>>,
}

/// Trims to `None`, so a field the vet blanked out is absent rather than an empty string.
fn blank_to_null(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn block_missing_fields(has_name: bool, has_content: bool) -> Vec<String> {
    missing_fields(&[("name", has_name), ("content", has_content)])
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
        r#"SELECT id, name, content AS "content: Jsonb<RichText>", content_text, archived, draft,
                  created_at, updated_at
           FROM text_block
           WHERE ($2 OR NOT archived)
             AND ($1::text IS NULL
                  OR name ILIKE '%' || $1 || '%'
                  OR content_text ILIKE '%' || $1 || '%')
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
                missing_fields: block_missing_fields(row.name.is_some(), row.content.is_some()),
                id: row.id,
                name: row.name,
                content: row.content.map(|document| document.0),
                content_text: row.content_text,
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
        r#"SELECT name IS NOT NULL AS "has_name!", content IS NOT NULL AS "has_content!", draft
           FROM text_block WHERE id = $1 FOR UPDATE"#,
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let name = body.name.map(blank_to_null);
    let content = body.content.map(rich_text::column);
    let draft = recompute_draft(
        current.draft,
        &[
            (
                "name",
                name.as_ref().map_or(current.has_name, Option::is_some),
            ),
            (
                "content",
                content
                    .as_ref()
                    .map_or(current.has_content, Option::is_some),
            ),
        ],
    )?;

    sqlx::query!(
        r#"UPDATE text_block SET
               name    = CASE WHEN $2 THEN $3 ELSE name END,
               content = CASE WHEN $4 THEN $5 ELSE content END,
               draft   = $6
           WHERE id = $1"#,
        id,
        name.is_some(),
        name.flatten(),
        content.is_some(),
        content.flatten() as Option<Jsonb<RichText>>,
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
        r#"SELECT id, name, content AS "content: Jsonb<RichText>", content_text, archived, draft,
                  created_at, updated_at
           FROM text_block WHERE id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(TextBlock {
        missing_fields: block_missing_fields(row.name.is_some(), row.content.is_some()),
        id: row.id,
        name: row.name,
        content: row.content.map(|document| document.0),
        content_text: row.content_text,
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
