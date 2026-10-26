//! Appointments — dated visits, each with exactly one treatment (T030).
//!
//! The treatment is made with the appointment and lives as long as it does: a second invoice
//! for the same visit is a second appointment (migration 0022). The appointment carries the
//! draft lifecycle — time, note, customer — and the treatment the clinical and billing record.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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
    /// Whose visit this is. Chosen here and carried down to the treatment, which is what
    /// limits the animals on offer to that customer's (FR-027). It can change until an animal
    /// is attached.
    pub customer_id: Option<i64>,
    /// For the header and the list, so the appointment says whose it is without a second
    /// request.
    pub customer_name: Option<String>,
    /// The animals on its treatment, by name — what tells two appointments of one customer
    /// apart in the list. A past visit keeps naming an animal that has since died.
    pub patient_names: Vec<String>,
    /// `true` while mandatory fields are missing — excluded from billing flows.
    pub draft: bool,
    /// Mandatory fields still empty, for the "incomplete — missing: …" hint.
    pub missing_fields: Vec<String>,
    /// The appointment's one treatment, made with it.
    pub treatment_id: i64,
    /// The treatment carries positions and no live invoice — work that has not been billed to
    /// anyone yet (FR-036).
    pub unbilled: bool,
    /// The live invoice's PDF no longer shows the treatment as it is (migration 0034).
    pub pdf_stale: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreateAppointment {
    pub starts_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
    pub customer_id: Option<i64>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchAppointment {
    #[serde(default, deserialize_with = "double_option")]
    pub starts_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub customer_id: Option<Option<i64>>,
}

/// Row shape shared by the queries below.
struct Row {
    id: i64,
    starts_at: Option<DateTime<Utc>>,
    note: Option<String>,
    customer_id: Option<i64>,
    customer_name: Option<String>,
    patient_names: Vec<String>,
    draft: bool,
    treatment_id: i64,
    unbilled: bool,
    pdf_stale: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for Appointment {
    fn from(row: Row) -> Self {
        Appointment {
            id: row.id,
            starts_at: row.starts_at,
            note: row.note,
            customer_id: row.customer_id,
            customer_name: row.customer_name,
            patient_names: row.patient_names,
            draft: row.draft,
            missing_fields: missing(row.starts_at.is_some()),
            treatment_id: row.treatment_id,
            unbilled: row.unbilled,
            pdf_stale: row.pdf_stale,
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
    params(ListQuery, AppointmentFilter),
    responses((status = 200, body = Vec<Appointment>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    Query(filter): Query<AppointmentFilter>,
) -> AppResult<Json<Vec<Appointment>>> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT appointment.id, appointment.starts_at, appointment.note, appointment.draft,
                  appointment.customer_id,
                  concat_ws(' ', customer.first_name, customer.last_name) AS customer_name,
                  appointment.created_at, appointment.updated_at,
                  treatment.id AS treatment_id,
                  ARRAY(SELECT patient.name FROM patient_treatment
                        JOIN patient ON patient.id = patient_treatment.patient_id
                        WHERE patient_treatment.treatment_id = treatment.id
                          AND patient.name IS NOT NULL
                        ORDER BY patient.name) AS "patient_names!",
                  (EXISTS (SELECT 1 FROM treatment_item
                           WHERE treatment_item.treatment_id = treatment.id)
                   AND NOT EXISTS (SELECT 1 FROM invoice
                                   WHERE invoice.treatment_id = treatment.id
                                     AND invoice.status <> 'cancelled')) AS "unbilled!",
                  COALESCE((SELECT invoice_pdf_out_of_date(invoice.id) FROM invoice
                            WHERE invoice.treatment_id = treatment.id
                              AND invoice.status <> 'cancelled'), false) AS "pdf_stale!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           LEFT JOIN customer ON customer.id = appointment.customer_id
           -- Whose visit and which animals, now that the list shows those instead of the note;
           -- the note is still searched.
           WHERE ($4::bigint IS NULL OR appointment.customer_id = $4)
             AND ($1::text IS NULL
                  OR appointment.note ILIKE '%' || $1 || '%'
                  OR concat_ws(' ', customer.first_name, customer.last_name) ILIKE '%' || $1 || '%'
                  OR EXISTS (SELECT 1 FROM patient_treatment
                             JOIN patient ON patient.id = patient_treatment.patient_id
                             WHERE patient_treatment.treatment_id = treatment.id
                               AND patient.name ILIKE '%' || $1 || '%'))
           ORDER BY appointment.starts_at DESC NULLS FIRST, appointment.id DESC
           LIMIT $2 OFFSET $3"#,
        query.search(),
        query.limit(),
        query.offset(),
        filter.customer_id,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(Appointment::from).collect()))
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AppointmentFilter {
    /// Only this customer's appointments — the customer page lists them.
    pub customer_id: Option<i64>,
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
    // Created as a draft immediately, so auto-save has a row to PATCH (research R4) — and with
    // its treatment, in the same transaction: there is never an appointment without one.
    let draft = body.starts_at.is_none();
    let mut transaction = state.pool.begin().await?;
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO appointment (starts_at, note, customer_id, draft)
         VALUES ($1, $2, $3, $4) RETURNING id",
        body.starts_at,
        body.note,
        body.customer_id,
        draft,
    )
    .fetch_one(&mut *transaction)
    .await?;
    let treatment_id: i64 = sqlx::query_scalar!(
        "INSERT INTO treatment (appointment_id, customer_id) VALUES ($1, $2) RETURNING id",
        id,
        body.customer_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    if body.customer_id.is_some() {
        treatments::attach_only_animal(&mut transaction, treatment_id).await?;
    }
    transaction.commit().await?;

    Ok(Json(load(&state.pool, id).await?))
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

pub(crate) async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Appointment> {
    let row = sqlx::query_as!(
        Row,
        r#"SELECT appointment.id, appointment.starts_at, appointment.note, appointment.draft,
                  appointment.customer_id,
                  concat_ws(' ', customer.first_name, customer.last_name) AS customer_name,
                  appointment.created_at, appointment.updated_at,
                  treatment.id AS treatment_id,
                  ARRAY(SELECT patient.name FROM patient_treatment
                        JOIN patient ON patient.id = patient_treatment.patient_id
                        WHERE patient_treatment.treatment_id = treatment.id
                          AND patient.name IS NOT NULL
                        ORDER BY patient.name) AS "patient_names!",
                  (EXISTS (SELECT 1 FROM treatment_item
                           WHERE treatment_item.treatment_id = treatment.id)
                   AND NOT EXISTS (SELECT 1 FROM invoice
                                   WHERE invoice.treatment_id = treatment.id
                                     AND invoice.status <> 'cancelled')) AS "unbilled!",
                  COALESCE((SELECT invoice_pdf_out_of_date(invoice.id) FROM invoice
                            WHERE invoice.treatment_id = treatment.id
                              AND invoice.status <> 'cancelled'), false) AS "pdf_stale!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           LEFT JOIN customer ON customer.id = appointment.customer_id
           WHERE appointment.id = $1"#,
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
        "SELECT starts_at, draft, customer_id FROM appointment WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // Merge the patch over the stored row before recomputing the draft flag.
    let starts_at = body.starts_at.unwrap_or(current.starts_at);
    let draft = recompute_draft(current.draft, &[("starts_at", starts_at.is_some())])?;

    // The customer can change until something has been entered on the treatment — a line, an
    // invoice, a reason or finding, a file. Until then its animals are only the default (the
    // customer's only one is attached by itself), so they go with the change rather than
    // locking a visit to a customer picked by mistake. The foreign keys would refuse a change
    // with animals on it (`patient_treatment_treatment_customer_fkey`); they come off first.
    let treatment_id: i64 =
        sqlx::query_scalar!("SELECT id FROM treatment WHERE appointment_id = $1", id)
            .fetch_one(&mut *transaction)
            .await?;
    let new_customer = body
        .customer_id
        .filter(|customer| *customer != current.customer_id);
    if new_customer.is_some() {
        if treatments::has_content(&mut transaction, treatment_id).await? {
            return Err(AppError::field("customer_id", "appointment.customerLocked"));
        }
        sqlx::query!(
            "DELETE FROM patient_treatment WHERE treatment_id = $1",
            treatment_id
        )
        .execute(&mut *transaction)
        .await?;
    }

    sqlx::query!(
        "UPDATE appointment SET
             starts_at   = CASE WHEN $2 THEN $3 ELSE starts_at END,
             note        = CASE WHEN $4 THEN $5 ELSE note END,
             customer_id = CASE WHEN $7 THEN $8 ELSE customer_id END,
             draft       = $6
         WHERE id = $1",
        id,
        body.starts_at.is_some(),
        starts_at,
        body.note.is_some(),
        body.note.flatten(),
        draft,
        body.customer_id.is_some(),
        body.customer_id.flatten(),
    )
    .execute(&mut *transaction)
    .await?;

    // A change from one customer to another reached the treatment through ON UPDATE CASCADE; a
    // first customer is no key change a cascade sees, so it is carried down here.
    if let Some(customer_id) = new_customer {
        sqlx::query!(
            "UPDATE treatment SET customer_id = $2
             WHERE id = $1 AND customer_id IS DISTINCT FROM $2",
            treatment_id,
            customer_id,
        )
        .execute(&mut *transaction)
        .await?;
        if customer_id.is_some() {
            treatments::attach_only_animal(&mut transaction, treatment_id).await?;
        }
    }

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    delete,
    operation_id = "deleteAppointment",
    path = "/api/appointments/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    responses(
        (status = 204, description = "Deleted"),
        (status = 409, description = "An invoice was written for its treatment")
    )
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let mut transaction = state.pool.begin().await?;

    // Appointments become permanent records once billed (FR-025). *Any* invoice counts, a
    // cancelled one included: cancelling burns the number instead of releasing it, so that row
    // is the only evidence the number was ever used, and `requirements/datamodel.md` is explicit
    // that data an invoiced treatment refers to is never hard-deleted.
    //
    // Excluding cancelled invoices used to let the delete through to the database, which refused
    // it anyway — `invoice.treatment_id` has no ON DELETE, so the cascade from `appointment` hit
    // a foreign key and came back as a 422 the screen had no branch for. The button did nothing
    // and said nothing. Asking the right question here is also what keeps
    // `drug_stock_movement.reverses_movement_id` out of it: those compensating rows only exist
    // once an invoice has been cancelled, which no longer reaches the delete at all.
    let invoiced: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM invoice
             JOIN treatment ON treatment.id = invoice.treatment_id
             WHERE treatment.appointment_id = $1
         )",
        id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    if invoiced.unwrap_or(false) {
        return Err(AppError::Conflict(
            "an invoice exists for the treatment of this appointment".to_owned(),
        ));
    }

    let deleted = sqlx::query!("DELETE FROM appointment WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // One transaction so the check and the delete see the same appointment; the foreign key
    // remains the backstop if an invoice is written between them.
    transaction.commit().await?;
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

    let source = sqlx::query!(
        "SELECT starts_at, note, customer_id FROM appointment WHERE id = $1",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // The copy lands today, keeping the time of day (FR-026).
    let starts_at = source.starts_at.map(today_with_time_of);
    let new_id: i64 = sqlx::query_scalar!(
        "INSERT INTO appointment (starts_at, note, customer_id, draft)
         VALUES ($1, $2, $3, $4) RETURNING id",
        starts_at,
        source.note,
        source.customer_id,
        starts_at.is_none(),
    )
    .fetch_one(&mut *transaction)
    .await?;

    // The copy's treatment is the source's, copied — animals, reasons, findings and lines.
    let source_treatment: i64 =
        sqlx::query_scalar!("SELECT id FROM treatment WHERE appointment_id = $1", id)
            .fetch_one(&mut *transaction)
            .await?;
    treatments::copy_treatment(
        &mut transaction,
        source_treatment,
        new_id,
        source.customer_id,
        body.price_mode,
        &state.config.travel_expenses,
    )
    .await?;

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
}
