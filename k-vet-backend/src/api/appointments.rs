//! Appointments — dated visits that hold treatments (T030).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{DuplicateRequest, ListQuery, double_option};
use crate::api::treatments;
use crate::domain::draft::recompute_draft;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Appointment {
    pub id: i64,
    /// Minute-resolution start; the date defaults to today in the UI, the time is typed.
    pub starts_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
    /// `true` while mandatory fields are missing — excluded from billing flows.
    pub draft: bool,
    /// Mandatory fields still empty, for the "incomplete — missing: …" hint.
    pub missing_fields: Vec<String>,
    pub treatment_count: i64,
    /// Treatments here that carry positions and no live invoice — work that has not been
    /// billed to anyone yet (FR-036).
    pub unbilled_treatment_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreateAppointment {
    pub starts_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchAppointment {
    #[serde(default, deserialize_with = "double_option")]
    pub starts_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
}

/// Row shape shared by the queries below.
struct Row {
    id: i64,
    starts_at: Option<DateTime<Utc>>,
    note: Option<String>,
    draft: bool,
    treatment_count: Option<i64>,
    unbilled_treatment_count: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for Appointment {
    fn from(row: Row) -> Self {
        Appointment {
            id: row.id,
            starts_at: row.starts_at,
            note: row.note,
            draft: row.draft,
            missing_fields: missing(row.starts_at.is_some()),
            treatment_count: row.treatment_count.unwrap_or(0),
            unbilled_treatment_count: row.unbilled_treatment_count.unwrap_or(0),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The completeness set of an appointment (data-model.md "Draft rows").
fn missing(has_starts_at: bool) -> Vec<String> {
    if has_starts_at {
        Vec::new()
    } else {
        vec!["starts_at".to_owned()]
    }
}

#[utoipa::path(
    get,
    operation_id = "listAppointments",
    path = "/api/appointments",
    tag = "appointments",
    params(ListQuery),
    responses((status = 200, body = Vec<Appointment>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<Appointment>>> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT appointment.id, appointment.starts_at, appointment.note, appointment.draft,
                  appointment.created_at, appointment.updated_at,
                  (SELECT count(*) FROM treatment WHERE treatment.appointment_id = appointment.id)
                      AS treatment_count,
                  (SELECT count(*) FROM treatment
                   WHERE treatment.appointment_id = appointment.id
                     AND EXISTS (SELECT 1 FROM treatment_item
                                 WHERE treatment_item.treatment_id = treatment.id)
                     AND NOT EXISTS (SELECT 1 FROM invoice
                                     WHERE invoice.treatment_id = treatment.id
                                       AND invoice.status <> 'cancelled'))
                      AS unbilled_treatment_count
           FROM appointment
           WHERE ($1::text IS NULL OR appointment.note ILIKE '%' || $1 || '%')
           ORDER BY appointment.starts_at DESC NULLS FIRST, appointment.id DESC
           LIMIT $2 OFFSET $3"#,
        query.search(),
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(Appointment::from).collect()))
}

#[utoipa::path(
    post,
    operation_id = "createAppointment",
    path = "/api/appointments",
    tag = "appointments",
    request_body = CreateAppointment,
    responses((status = 200, body = Appointment))
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateAppointment>,
) -> AppResult<Json<Appointment>> {
    // Created as a draft immediately, so auto-save has a row to PATCH (research R4).
    let draft = body.starts_at.is_none();
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO appointment (starts_at, note, draft)
           VALUES ($1, $2, $3)
           RETURNING id, starts_at, note, draft, created_at, updated_at,
                     0::bigint AS treatment_count, 0::bigint AS unbilled_treatment_count"#,
        body.starts_at,
        body.note,
        draft,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row.into()))
}

#[utoipa::path(
    get,
    operation_id = "getAppointment",
    path = "/api/appointments/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Appointment), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Appointment>> {
    Ok(Json(load(&state.pool, id).await?))
}

async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Appointment> {
    let row = sqlx::query_as!(
        Row,
        r#"SELECT appointment.id, appointment.starts_at, appointment.note, appointment.draft,
                  appointment.created_at, appointment.updated_at,
                  (SELECT count(*) FROM treatment WHERE treatment.appointment_id = appointment.id)
                      AS treatment_count,
                  (SELECT count(*) FROM treatment
                   WHERE treatment.appointment_id = appointment.id
                     AND EXISTS (SELECT 1 FROM treatment_item
                                 WHERE treatment_item.treatment_id = treatment.id)
                     AND NOT EXISTS (SELECT 1 FROM invoice
                                     WHERE invoice.treatment_id = treatment.id
                                       AND invoice.status <> 'cancelled'))
                      AS unbilled_treatment_count
           FROM appointment WHERE appointment.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

#[utoipa::path(
    patch,
    operation_id = "patchAppointment",
    path = "/api/appointments/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    request_body = PatchAppointment,
    responses((status = 200, body = Appointment), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchAppointment>,
) -> AppResult<Json<Appointment>> {
    let mut transaction = state.pool.begin().await?;

    let current = sqlx::query!(
        "SELECT starts_at, draft FROM appointment WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // Merge the patch over the stored row before recomputing the draft flag.
    let starts_at = body.starts_at.unwrap_or(current.starts_at);
    let draft = recompute_draft(current.draft, &[("starts_at", starts_at.is_some())])?;

    let row = sqlx::query_as!(
        Row,
        r#"UPDATE appointment SET
               starts_at = CASE WHEN $2 THEN $3 ELSE starts_at END,
               note      = CASE WHEN $4 THEN $5 ELSE note END,
               draft     = $6
           WHERE id = $1
           RETURNING id, starts_at, note, draft, created_at, updated_at,
                     (SELECT count(*) FROM treatment WHERE treatment.appointment_id = appointment.id)
                         AS treatment_count,
                     (SELECT count(*) FROM treatment
                      WHERE treatment.appointment_id = appointment.id
                        AND EXISTS (SELECT 1 FROM treatment_item
                                    WHERE treatment_item.treatment_id = treatment.id)
                        AND NOT EXISTS (SELECT 1 FROM invoice
                                        WHERE invoice.treatment_id = treatment.id
                                          AND invoice.status <> 'cancelled'))
                         AS unbilled_treatment_count"#,
        id,
        body.starts_at.is_some(),
        starts_at,
        body.note.is_some(),
        body.note.flatten(),
        draft,
    )
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(row.into()))
}

#[utoipa::path(
    delete,
    operation_id = "deleteAppointment",
    path = "/api/appointments/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    responses(
        (status = 204, description = "Deleted"),
        (status = 409, description = "An invoice exists for one of its treatments")
    )
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    // Appointments become permanent records once billed (FR-025).
    let invoiced: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM invoice
             JOIN treatment ON treatment.id = invoice.treatment_id
             WHERE treatment.appointment_id = $1 AND invoice.status <> 'cancelled'
         )",
        id,
    )
    .fetch_one(&state.pool)
    .await?;
    if invoiced.unwrap_or(false) {
        return Err(AppError::Conflict(
            "an invoice exists for a treatment of this appointment".to_owned(),
        ));
    }

    let deleted = sqlx::query!("DELETE FROM appointment WHERE id = $1", id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    operation_id = "duplicateAppointment",
    path = "/api/appointments/{id}/duplicate",
    tag = "appointments",
    params(("id" = i64, Path,)),
    request_body = DuplicateRequest,
    responses((status = 200, body = Appointment), (status = 404))
)]
pub async fn duplicate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<DuplicateRequest>,
) -> AppResult<Json<Appointment>> {
    let mut transaction = state.pool.begin().await?;

    let source = sqlx::query!("SELECT starts_at, note FROM appointment WHERE id = $1", id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;

    // The copy lands today, keeping the time of day (FR-026).
    let starts_at = source.starts_at.map(today_with_time_of);
    let new_id: i64 = sqlx::query_scalar!(
        "INSERT INTO appointment (starts_at, note, draft) VALUES ($1, $2, $3) RETURNING id",
        starts_at,
        source.note,
        starts_at.is_none(),
    )
    .fetch_one(&mut *transaction)
    .await?;

    let treatments = sqlx::query_scalar!(
        "SELECT id FROM treatment WHERE appointment_id = $1 ORDER BY id",
        id
    )
    .fetch_all(&mut *transaction)
    .await?;
    for treatment_id in treatments {
        treatments::copy_treatment(
            &mut transaction,
            treatment_id,
            new_id,
            body.price_mode,
            &state.config.travel_expenses,
        )
        .await?;
    }

    transaction.commit().await?;
    Ok(Json(load(&state.pool, new_id).await?))
}

/// Keeps the local time of day but moves the date to today.
fn today_with_time_of(source: DateTime<Utc>) -> DateTime<Utc> {
    let local = source.with_timezone(&Local);
    let today = Local::now().date_naive();
    let time: NaiveTime = local.time();
    match Local.from_local_datetime(&today.and_time(time)).earliest() {
        Some(moved) => moved.with_timezone(&Utc),
        // Skipped local time (DST change): fall back to the original time of day in UTC.
        None => today.and_time(time).and_utc(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/appointments", get(list).post(create))
        .route(
            "/appointments/{id}",
            get(detail).patch(patch).delete(delete),
        )
        .route("/appointments/{id}/duplicate", post(duplicate))
        .route(
            "/appointments/{id}/treatments",
            get(treatments::list_for_appointment).post(treatments::create_for_appointment),
        )
}
