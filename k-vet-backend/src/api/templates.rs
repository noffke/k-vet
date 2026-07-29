//! Treatment templates: named, ordered sets of lines for quick entry (T063, FR-024).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, MoveDirection, MoveRequest, double_option};
use crate::api::suppliers::blank_to_null;
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::domain::enums::TemplateItemKind;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Template {
    pub id: i64,
    pub name: Option<String>,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    pub item_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TemplateItem {
    pub id: i64,
    pub template_id: i64,
    pub position: i32,
    pub kind: TemplateItemKind,
    pub drug_packaging_id: Option<i64>,
    pub service_id: Option<i64>,
    /// Current catalog name — a template stores the reference, not a price.
    pub name: String,
    pub quantity: Decimal,
    pub unit: Option<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchTemplate {
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTemplateItem {
    pub kind: TemplateItemKind,
    pub drug_packaging_id: Option<i64>,
    pub service_id: Option<i64>,
    #[serde(default = "one")]
    pub quantity: Decimal,
}

fn one() -> Decimal {
    Decimal::ONE
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchTemplateItem {
    pub quantity: Option<Decimal>,
}

#[utoipa::path(
    get,
    path = "/api/treatment-templates",
    operation_id = "listTemplates",
    tag = "templates",
    params(ListQuery),
    responses((status = 200, body = Vec<Template>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<Template>>> {
    let rows = sqlx::query!(
        r#"SELECT template.id, template.name, template.archived, template.draft,
                  template.created_at, template.updated_at,
                  (SELECT count(*) FROM treatment_template_item
                    WHERE template_id = template.id) AS item_count
           FROM treatment_template template
           WHERE ($2 OR NOT template.archived)
             AND ($1::text IS NULL OR template.name ILIKE '%' || $1 || '%')
           ORDER BY template.name NULLS LAST, template.id
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
            .map(|row| Template {
                missing_fields: missing_fields(&[("name", row.name.is_some())])
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                id: row.id,
                name: row.name,
                archived: row.archived,
                draft: row.draft,
                item_count: row.item_count.unwrap_or(0),
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/treatment-templates",
    operation_id = "createTemplate",
    tag = "templates",
    responses((status = 200, description = "An empty draft", body = Template))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<Template>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO treatment_template DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/treatment-templates/{id}",
    operation_id = "getTemplate",
    tag = "templates",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Template), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Template>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/treatment-templates/{id}",
    operation_id = "patchTemplate",
    tag = "templates",
    params(("id" = i64, Path,)),
    request_body = PatchTemplate,
    responses((status = 200, body = Template), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchTemplate>,
) -> AppResult<Json<Template>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT name, draft FROM treatment_template WHERE id = $1 FOR UPDATE",
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
        "UPDATE treatment_template SET
             name  = CASE WHEN $2 THEN $3 ELSE name END,
             draft = $4
         WHERE id = $1",
        id,
        body.name.is_some(),
        blank_to_null(&body.name),
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/treatment-templates/{id}/archive",
    operation_id = "archiveTemplate",
    tag = "templates",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Template))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Template>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/treatment-templates/{id}/unarchive",
    operation_id = "unarchiveTemplate",
    tag = "templates",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Template))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Template>> {
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<Template>> {
    let updated = sqlx::query!(
        "UPDATE treatment_template SET archived = $2 WHERE id = $1",
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

#[utoipa::path(
    get,
    path = "/api/treatment-templates/{id}/items",
    operation_id = "listTemplateItems",
    tag = "templates",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<TemplateItem>))
)]
pub async fn list_items(
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
) -> AppResult<Json<Vec<TemplateItem>>> {
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_items(&mut connection, template_id).await?))
}

#[utoipa::path(
    post,
    path = "/api/treatment-templates/{id}/items",
    operation_id = "createTemplateItem",
    tag = "templates",
    params(("id" = i64, Path,)),
    request_body = CreateTemplateItem,
    responses((status = 200, body = Vec<TemplateItem>), (status = 422))
)]
pub async fn create_item(
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
    Json(body): Json<CreateTemplateItem>,
) -> AppResult<Json<Vec<TemplateItem>>> {
    if body.quantity <= Decimal::ZERO {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }

    let mut transaction = state.pool.begin().await?;
    let position: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM treatment_template_item
         WHERE template_id = $1",
        template_id,
    )
    .fetch_one(&mut *transaction)
    .await?
    .unwrap_or(1);

    // A template stores a reference and a quantity — never a price. Prices are pinned when
    // the template is applied to a treatment.
    let (packaging_id, service_id, unit) = match body.kind {
        TemplateItemKind::DrugPackaging => {
            let id = body
                .drug_packaging_id
                .ok_or_else(|| AppError::field("drug_packaging_id", "field.required"))?;
            let unit = sqlx::query_scalar!("SELECT unit FROM drug_packaging WHERE id = $1", id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or_else(|| AppError::field("drug_packaging_id", "record.notFound"))?;
            (Some(id), None, unit)
        }
        TemplateItemKind::Service => {
            let id = body
                .service_id
                .ok_or_else(|| AppError::field("service_id", "field.required"))?;
            (None, Some(id), None)
        }
    };

    sqlx::query!(
        "INSERT INTO treatment_template_item
             (template_id, position, kind, drug_packaging_id, service_id, quantity, unit)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        template_id,
        position,
        body.kind as TemplateItemKind,
        packaging_id,
        service_id,
        body.quantity,
        unit,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_items(&mut connection, template_id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/template-items/{id}",
    operation_id = "patchTemplateItem",
    tag = "templates",
    params(("id" = i64, Path,)),
    request_body = PatchTemplateItem,
    responses((status = 200, body = Vec<TemplateItem>), (status = 404), (status = 422))
)]
pub async fn patch_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchTemplateItem>,
) -> AppResult<Json<Vec<TemplateItem>>> {
    if let Some(quantity) = body.quantity
        && quantity <= Decimal::ZERO
    {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }

    let template_id: i64 = sqlx::query_scalar!(
        "UPDATE treatment_template_item SET quantity = COALESCE($2, quantity)
         WHERE id = $1 RETURNING template_id",
        id,
        body.quantity,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_items(&mut connection, template_id).await?))
}

#[utoipa::path(
    delete,
    path = "/api/template-items/{id}",
    operation_id = "deleteTemplateItem",
    tag = "templates",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 404))
)]
pub async fn delete_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT template_id, position FROM treatment_template_item WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query!("DELETE FROM treatment_template_item WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;
    // Keep the positions 1..n so the order stays obvious.
    sqlx::query!(
        "UPDATE treatment_template_item SET position = position - 1
         WHERE template_id = $1 AND position > $2",
        current.template_id,
        current.position,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/template-items/{id}/move",
    operation_id = "moveTemplateItem",
    tag = "templates",
    params(("id" = i64, Path,)),
    request_body = MoveRequest,
    responses((status = 200, body = Vec<TemplateItem>), (status = 404))
)]
pub async fn move_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<MoveRequest>,
) -> AppResult<Json<Vec<TemplateItem>>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        "SELECT template_id, position FROM treatment_template_item WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    reorder(
        &mut transaction,
        current.template_id,
        id,
        current.position,
        body.direction,
    )
    .await?;

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        load_items(&mut connection, current.template_id).await?,
    ))
}

/// Moves a template line one step up or down, or to the top or bottom (FR-024).
///
/// `UNIQUE (template_id, position)` is deferred, so positions may collide inside the
/// transaction and are checked once at commit.
async fn reorder(
    transaction: &mut Transaction<'_, Postgres>,
    template_id: i64,
    id: i64,
    position: i32,
    direction: MoveDirection,
) -> AppResult<()> {
    let max_position: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) FROM treatment_template_item WHERE template_id = $1",
        template_id,
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
            sqlx::query!(
                "UPDATE treatment_template_item SET position = $1
                 WHERE template_id = $2 AND position = $3",
                position,
                template_id,
                target,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!(
                "UPDATE treatment_template_item SET position = $1 WHERE id = $2",
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
                "UPDATE treatment_template_item SET position = position + 1
                 WHERE template_id = $1 AND position < $2",
                template_id,
                position,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!(
                "UPDATE treatment_template_item SET position = 1 WHERE id = $1",
                id
            )
            .execute(&mut **transaction)
            .await?;
        }
        MoveDirection::Bottom => {
            if position == max_position {
                return Ok(());
            }
            sqlx::query!(
                "UPDATE treatment_template_item SET position = position - 1
                 WHERE template_id = $1 AND position > $2",
                template_id,
                position,
            )
            .execute(&mut **transaction)
            .await?;
            sqlx::query!(
                "UPDATE treatment_template_item SET position = $1 WHERE id = $2",
                max_position,
                id,
            )
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Template> {
    let row = sqlx::query!(
        r#"SELECT template.id, template.name, template.archived, template.draft,
                  template.created_at, template.updated_at,
                  (SELECT count(*) FROM treatment_template_item
                    WHERE template_id = template.id) AS item_count
           FROM treatment_template template WHERE template.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Template {
        missing_fields: missing_fields(&[("name", row.name.is_some())])
            .into_iter()
            .map(str::to_owned)
            .collect(),
        id: row.id,
        name: row.name,
        archived: row.archived,
        draft: row.draft,
        item_count: row.item_count.unwrap_or(0),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn load_items(
    connection: &mut PgConnection,
    template_id: i64,
) -> AppResult<Vec<TemplateItem>> {
    let rows = sqlx::query!(
        r#"SELECT item.id, item.template_id, item.position,
                  item.kind AS "kind: TemplateItemKind", item.drug_packaging_id,
                  item.service_id, item.quantity, item.unit,
                  drug.name AS "drug_name?", service.name AS "service_name?",
                  packaging.quantity AS "packaging_quantity?"
           FROM treatment_template_item item
           LEFT JOIN drug_packaging packaging ON packaging.id = item.drug_packaging_id
           LEFT JOIN drug ON drug.id = packaging.drug_id
           LEFT JOIN service ON service.id = item.service_id
           WHERE item.template_id = $1
           ORDER BY item.position"#,
        template_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TemplateItem {
            name: match (&row.drug_name, &row.service_name, row.packaging_quantity) {
                (Some(drug), _, Some(quantity)) => {
                    format!(
                        "{drug} · {} {}",
                        crate::pdf::number_de(quantity),
                        row.unit.clone().unwrap_or_default()
                    )
                }
                (Some(drug), _, None) => drug.clone(),
                (None, Some(service), _) => service.clone(),
                _ => String::new(),
            },
            id: row.id,
            template_id: row.template_id,
            position: row.position,
            kind: row.kind,
            drug_packaging_id: row.drug_packaging_id,
            service_id: row.service_id,
            quantity: row.quantity,
            unit: row.unit,
        })
        .collect())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/treatment-templates", get(list).post(create))
        .route("/treatment-templates/{id}", get(detail).patch(patch))
        .route("/treatment-templates/{id}/archive", post(archive))
        .route("/treatment-templates/{id}/unarchive", post(unarchive))
        .route(
            "/treatment-templates/{id}/items",
            get(list_items).post(create_item),
        )
        .route(
            "/template-items/{id}",
            axum::routing::patch(patch_item).delete(delete_item),
        )
        .route("/template-items/{id}/move", post(move_item))
}
