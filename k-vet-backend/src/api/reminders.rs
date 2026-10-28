//! "Mitbringen" (migration 0035): drugs the vet is asked to bring to a visit, and each day's
//! Packliste of everything to take along.
//!
//! A reminder is not a line. Nothing is dispensed and no animal is named until the vet takes it
//! into the treatment on the day ("Als Position übernehmen"), and that goes through the same
//! `insert_pinned_item` a picked drug does: prices pinned, the dispense FEFO, a shortfall
//! refused with `item.stockShort`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeDelta, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::api::common::double_option;
use crate::api::treatment_items;
use crate::domain::enums::TreatmentItemKind;
use crate::domain::stock;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Reminder {
    pub id: i64,
    pub appointment_id: i64,
    pub drug_packaging_id: i64,
    pub drug_id: i64,
    /// The drug's name.
    pub name: String,
    pub unit: Option<String>,
    /// One packaging's size in its unit — `10` (ml).
    pub size: Option<Decimal>,
    /// Packagings, as a line counts them.
    pub quantity: Decimal,
    pub note: Option<String>,
    /// Ticked off on the Packliste.
    pub packed: bool,
    pub position: i32,
    /// The position it became; absent while it is a reminder.
    pub taken_item_id: Option<i64>,
    /// The drug's lots together hold less than this reminder needs.
    pub stock_short: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateReminder {
    pub drug_packaging_id: i64,
    /// Packagings; one when left out.
    pub quantity: Option<Decimal>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchReminder {
    pub quantity: Option<Decimal>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
    pub packed: Option<bool>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct TakeReminder {
    /// The animal it is for. Left out, the visit's only animal (FR-028).
    pub patient_treatment_id: Option<i64>,
}

/// One packaging's total for the day, and whether the shelf covers the drug.
#[derive(Debug, Serialize, ToSchema)]
pub struct PackingTotal {
    pub drug_packaging_id: i64,
    pub drug_id: i64,
    pub name: String,
    pub unit: Option<String>,
    pub size: Option<Decimal>,
    /// Packagings, summed over the day's appointments.
    pub quantity: Decimal,
    /// What the drug's lots hold together, in base units.
    pub in_stock: Decimal,
    /// The day needs more of the drug than its lots hold — counted over all of the drug's
    /// packagings, since they draw from the same lots.
    pub short: bool,
}

/// An appointment of the day with what to bring to it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PackingAppointment {
    pub appointment_id: i64,
    pub starts_at: DateTime<Utc>,
    pub customer_name: Option<String>,
    pub patients: Vec<String>,
    pub reminders: Vec<Reminder>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackingList {
    pub date: NaiveDate,
    pub totals: Vec<PackingTotal>,
    pub appointments: Vec<PackingAppointment>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PackingListQuery {
    /// The day (`yyyy-MM-dd`, the practice's own); today when left out.
    pub date: Option<NaiveDate>,
}

/// Which reminders `load` reads: one, an appointment's, or a day's open ones.
#[derive(Debug, Default)]
struct Filter {
    id: Option<i64>,
    appointment_id: Option<i64>,
    /// From this instant on…
    from: Option<DateTime<Utc>>,
    /// …until this one, and only the ones not yet taken.
    until: Option<DateTime<Utc>>,
}

async fn load(connection: &mut PgConnection, filter: Filter) -> AppResult<Vec<Reminder>> {
    let rows = sqlx::query!(
        r#"WITH stock AS (
               SELECT drug_id, SUM(remaining) AS remaining FROM lot_remaining GROUP BY drug_id
           )
           SELECT reminder.id, reminder.appointment_id, reminder.drug_packaging_id,
                  drug.id AS drug_id, COALESCE(drug.name, '') AS "name!",
                  packaging.unit, packaging.quantity AS size,
                  reminder.quantity, reminder.note, reminder.packed, reminder.position,
                  reminder.taken_item_id,
                  (original.id IS NOT NULL
                   AND COALESCE(stock.remaining, 0)
                       < reminder.quantity * COALESCE(packaging.quantity, 1)) AS "stock_short!"
           FROM appointment_reminder reminder
           JOIN appointment ON appointment.id = reminder.appointment_id
           JOIN drug_packaging packaging ON packaging.id = reminder.drug_packaging_id
           JOIN drug ON drug.id = packaging.drug_id
           LEFT JOIN drug_packaging original
                  ON original.drug_id = drug.id AND original.kind = 'original'
           LEFT JOIN stock ON stock.drug_id = drug.id
           WHERE ($1::bigint IS NULL OR reminder.id = $1)
             AND ($2::bigint IS NULL OR reminder.appointment_id = $2)
             AND ($3::timestamptz IS NULL
                  OR (appointment.starts_at >= $3 AND appointment.starts_at < $4
                      AND reminder.taken_item_id IS NULL))
           ORDER BY appointment.starts_at, reminder.appointment_id, reminder.position"#,
        filter.id,
        filter.appointment_id,
        filter.from,
        filter.until,
    )
    .fetch_all(&mut *connection)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Reminder {
            id: row.id,
            appointment_id: row.appointment_id,
            drug_packaging_id: row.drug_packaging_id,
            drug_id: row.drug_id,
            name: row.name,
            unit: row.unit,
            size: row.size,
            quantity: row.quantity,
            note: row.note,
            packed: row.packed,
            position: row.position,
            taken_item_id: row.taken_item_id,
            stock_short: row.stock_short,
        })
        .collect())
}

async fn load_one(connection: &mut PgConnection, id: i64) -> AppResult<Reminder> {
    load(
        connection,
        Filter {
            id: Some(id),
            ..Filter::default()
        },
    )
    .await?
    .into_iter()
    .next()
    .ok_or(AppError::NotFound)
}

/// Trims to `None`, so a note the vet blanked out is absent rather than an empty string.
fn blank_to_null(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

#[utoipa::path(
    get,
    operation_id = "listReminders",
    path = "/api/appointments/{id}/reminders",
    tag = "appointments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<Reminder>))
)]
pub async fn list(
    State(state): State<AppState>,
    Path(appointment_id): Path<i64>,
) -> AppResult<Json<Vec<Reminder>>> {
    let mut connection = state.pool.acquire().await?;
    let filter = Filter {
        appointment_id: Some(appointment_id),
        ..Filter::default()
    };
    Ok(Json(load(&mut connection, filter).await?))
}

#[utoipa::path(
    post,
    operation_id = "createReminder",
    path = "/api/appointments/{id}/reminders",
    tag = "appointments",
    params(("id" = i64, Path,)),
    request_body = CreateReminder,
    responses((status = 200, body = Reminder), (status = 404), (status = 422))
)]
pub async fn create(
    State(state): State<AppState>,
    Path(appointment_id): Path<i64>,
    Json(body): Json<CreateReminder>,
) -> AppResult<Json<Reminder>> {
    let quantity = body.quantity.unwrap_or(Decimal::ONE);
    if quantity <= Decimal::ZERO {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }
    let mut transaction = state.pool.begin().await?;
    sqlx::query_scalar!(
        "SELECT id FROM appointment WHERE id = $1 FOR UPDATE",
        appointment_id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    // What the picker offers: a complete packaging of a complete drug, neither archived.
    let available = sqlx::query_scalar!(
        "SELECT true FROM drug_packaging packaging JOIN drug ON drug.id = packaging.drug_id
          WHERE packaging.id = $1
            AND NOT packaging.draft AND NOT packaging.archived
            AND NOT drug.draft AND NOT drug.archived",
        body.drug_packaging_id,
    )
    .fetch_optional(&mut *transaction)
    .await?;
    if available.is_none() {
        return Err(AppError::field(
            "drug_packaging_id",
            "reminder.drugUnavailable",
        ));
    }
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO appointment_reminder (appointment_id, drug_packaging_id, quantity, position)
         VALUES ($1, $2, $3,
                 (SELECT COALESCE(MAX(position), 0) + 1 FROM appointment_reminder
                   WHERE appointment_id = $1))
         RETURNING id",
        appointment_id,
        body.drug_packaging_id,
        quantity,
    )
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_one(&mut connection, id).await?))
}

#[utoipa::path(
    patch,
    operation_id = "patchReminder",
    path = "/api/reminders/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    request_body = PatchReminder,
    responses((status = 200, body = Reminder), (status = 404), (status = 422))
)]
pub async fn patch_reminder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchReminder>,
) -> AppResult<Json<Reminder>> {
    if body
        .quantity
        .is_some_and(|quantity| quantity <= Decimal::ZERO)
    {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }
    let updated = sqlx::query_scalar!(
        "UPDATE appointment_reminder SET
             quantity = COALESCE($2, quantity),
             note     = CASE WHEN $3 THEN $4 ELSE note END,
             packed   = COALESCE($5, packed)
         WHERE id = $1
         RETURNING id",
        id,
        body.quantity,
        body.note.is_some(),
        blank_to_null(body.note.flatten()),
        body.packed,
    )
    .fetch_optional(&state.pool)
    .await?;
    updated.ok_or(AppError::NotFound)?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_one(&mut connection, id).await?))
}

#[utoipa::path(
    delete,
    operation_id = "deleteReminder",
    path = "/api/reminders/{id}",
    tag = "appointments",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 404))
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let mut transaction = state.pool.begin().await?;
    let appointment_id = sqlx::query_scalar!(
        "DELETE FROM appointment_reminder WHERE id = $1 RETURNING appointment_id",
        id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    // Back to 1..n; the unique index is deferred, so the collisions on the way do not count.
    sqlx::query!(
        "UPDATE appointment_reminder AS reminder
         SET position = ranked.rank
         FROM (SELECT id, (row_number() OVER (ORDER BY position))::int AS rank
               FROM appointment_reminder WHERE appointment_id = $1) AS ranked
         WHERE reminder.id = ranked.id AND reminder.position <> ranked.rank",
        appointment_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Turns a reminder into a position: the drug, the quantity, on the animal named (or the visit's
/// only one), dispensed from the lots as if picked — and the reminder points at it from then on.
#[utoipa::path(
    post,
    operation_id = "takeReminder",
    path = "/api/reminders/{id}/take",
    tag = "appointments",
    params(("id" = i64, Path,)),
    request_body = TakeReminder,
    responses(
        (status = 200, description = "The reminder, now pointing at its position", body = Reminder),
        (status = 404),
        (status = 409, description = "The treatment's invoice is accepted"),
        (status = 422, description = "Taken already, no animal to give it to, or short of stock")
    )
)]
pub async fn take(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TakeReminder>,
) -> AppResult<Json<Reminder>> {
    let mut transaction = state.pool.begin().await?;
    let reminder = sqlx::query!(
        r#"SELECT reminder.drug_packaging_id, reminder.quantity, reminder.taken_item_id,
                  treatment.id AS treatment_id
           FROM appointment_reminder reminder
           JOIN treatment ON treatment.appointment_id = reminder.appointment_id
           WHERE reminder.id = $1
           FOR UPDATE OF reminder"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if reminder.taken_item_id.is_some() {
        return Err(AppError::field("reminder", "reminder.alreadyTaken"));
    }
    stock::ensure_editable(&mut transaction, reminder.treatment_id).await?;
    crate::api::treatments::ensure_open(&mut transaction, reminder.treatment_id).await?;

    let item_id = treatment_items::insert_pinned_item(
        &mut transaction,
        reminder.treatment_id,
        TreatmentItemKind::DrugPackaging,
        reminder.drug_packaging_id,
        reminder.quantity,
        body.patient_treatment_id.into(),
        None,
        None,
        false,
        None,
        &state.config.travel_expenses,
    )
    .await?;
    sqlx::query!(
        "UPDATE appointment_reminder SET taken_item_id = $2 WHERE id = $1",
        id,
        item_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load_one(&mut connection, id).await?))
}

/// The practice's midnight starting `date`, as an instant.
fn start_of(date: NaiveDate) -> DateTime<Utc> {
    Local
        .from_local_datetime(&date.and_time(NaiveTime::MIN))
        .earliest()
        .map_or_else(
            || date.and_time(NaiveTime::MIN).and_utc(),
            |start| start.with_timezone(&Utc),
        )
}

/// What to pack for a day: every drug the day's appointments are to get, added up per
/// packaging and checked against the shelf, and the same items appointment by appointment.
/// Reminders already taken into a position are not on it.
#[utoipa::path(
    get,
    operation_id = "getPackingList",
    path = "/api/packing-list",
    tag = "appointments",
    params(PackingListQuery),
    responses((status = 200, body = PackingList))
)]
pub async fn packing_list(
    State(state): State<AppState>,
    Query(query): Query<PackingListQuery>,
) -> AppResult<Json<PackingList>> {
    let date = query.date.unwrap_or_else(|| Local::now().date_naive());
    let from = start_of(date);
    let until = start_of(date + TimeDelta::days(1));
    let mut connection = state.pool.acquire().await?;

    let totals = sqlx::query!(
        r#"WITH day AS (
               SELECT reminder.drug_packaging_id, reminder.quantity,
                      packaging.drug_id, packaging.quantity AS size
               FROM appointment_reminder reminder
               JOIN appointment ON appointment.id = reminder.appointment_id
               JOIN drug_packaging packaging ON packaging.id = reminder.drug_packaging_id
               WHERE appointment.starts_at >= $1 AND appointment.starts_at < $2
                 AND reminder.taken_item_id IS NULL
           ),
           need AS (
               SELECT drug_id, SUM(quantity * COALESCE(size, 1)) AS base FROM day GROUP BY drug_id
           ),
           stock AS (
               SELECT drug_id, SUM(remaining) AS remaining FROM lot_remaining GROUP BY drug_id
           )
           SELECT day.drug_packaging_id, day.drug_id, COALESCE(drug.name, '') AS "name!",
                  packaging.unit, packaging.quantity AS size,
                  SUM(day.quantity) AS "quantity!",
                  COALESCE(MAX(stock.remaining), 0) AS "in_stock!",
                  (bool_or(original.id IS NOT NULL)
                   AND COALESCE(MAX(stock.remaining), 0) < MAX(need.base)) AS "short!"
           FROM day
           JOIN drug ON drug.id = day.drug_id
           JOIN drug_packaging packaging ON packaging.id = day.drug_packaging_id
           JOIN need ON need.drug_id = day.drug_id
           LEFT JOIN drug_packaging original
                  ON original.drug_id = day.drug_id AND original.kind = 'original'
           LEFT JOIN stock ON stock.drug_id = day.drug_id
           GROUP BY day.drug_packaging_id, day.drug_id, drug.name, packaging.unit,
                    packaging.quantity
           ORDER BY drug.name, packaging.quantity"#,
        from,
        until,
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(|row| PackingTotal {
        drug_packaging_id: row.drug_packaging_id,
        drug_id: row.drug_id,
        name: row.name,
        unit: row.unit,
        size: row.size,
        quantity: row.quantity,
        in_stock: row.in_stock,
        short: row.short,
    })
    .collect();

    let visits = sqlx::query!(
        r#"SELECT appointment.id, appointment.starts_at AS "starts_at!",
                  NULLIF(concat_ws(' ', customer.first_name, customer.last_name), '')
                      AS customer_name,
                  ARRAY(SELECT patient.name FROM patient_treatment
                        JOIN patient ON patient.id = patient_treatment.patient_id
                        WHERE patient_treatment.treatment_id = treatment.id
                          AND patient.name IS NOT NULL
                        ORDER BY patient.name) AS "patients!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           LEFT JOIN customer ON customer.id = appointment.customer_id
           WHERE appointment.starts_at >= $1 AND appointment.starts_at < $2
             AND EXISTS (SELECT 1 FROM appointment_reminder reminder
                         WHERE reminder.appointment_id = appointment.id
                           AND reminder.taken_item_id IS NULL)
           ORDER BY appointment.starts_at, appointment.id"#,
        from,
        until,
    )
    .fetch_all(&mut *connection)
    .await?;
    let reminders = load(
        &mut connection,
        Filter {
            from: Some(from),
            until: Some(until),
            ..Filter::default()
        },
    )
    .await?;
    let appointments = visits
        .into_iter()
        .map(|visit| PackingAppointment {
            appointment_id: visit.id,
            starts_at: visit.starts_at,
            customer_name: visit.customer_name,
            patients: visit.patients,
            reminders: reminders
                .iter()
                .filter(|reminder| reminder.appointment_id == visit.id)
                .cloned()
                .collect(),
        })
        .collect();

    Ok(Json(PackingList {
        date,
        totals,
        appointments,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/appointments/{id}/reminders", get(list).post(create))
        .route("/reminders/{id}", patch(patch_reminder).delete(delete))
        .route("/reminders/{id}/take", post(take))
        .route("/packing-list", get(packing_list))
}
