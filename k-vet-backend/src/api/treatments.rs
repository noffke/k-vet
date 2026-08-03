//! Treatments — the medical record of one visit, and the source of every invoice (T031).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{DuplicateRequest, PriceMode, double_option};
use crate::api::treatment_items::{self, CatalogLine, TreatmentItem};
use crate::config::TravelExpenseConfig;
use crate::domain::enums::{InvoiceStatus, TreatmentItemKind};
use crate::domain::money;
use crate::domain::stock;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Treatment {
    pub id: i64,
    pub appointment_id: i64,
    pub starts_at: Option<DateTime<Utc>>,
    pub treatment_reason: Option<String>,
    pub finding: Option<String>,
    pub patients: Vec<TreatmentPatient>,
    /// Derived from the patients — the invoice's customer.
    pub customer_id: Option<i64>,
    /// The customer's email addresses, offered as invoice recipients (FR-031).
    pub customer_emails: Vec<String>,
    /// The live invoice of this treatment, or the most recent cancelled one — the vet has
    /// to see that the last attempt was cancelled, and can then bill again.
    pub invoice: Option<TreatmentInvoice>,
    /// `true` once the invoice is accepted: lines and stock movements are frozen.
    pub frozen: bool,
    pub total_gross: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentPatient {
    pub patient_id: i64,
    pub name: String,
    pub customer_id: i64,
    pub warning_remark: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentInvoice {
    pub id: i64,
    pub invoice_number: String,
    pub status: InvoiceStatus,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreateTreatment {
    pub treatment_reason: Option<String>,
    pub finding: Option<String>,
    /// Patients to attach right away; all must belong to one customer.
    #[serde(default)]
    pub patient_ids: Vec<i64>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchTreatment {
    #[serde(default, deserialize_with = "double_option")]
    pub treatment_reason: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub finding: Option<Option<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddPatient {
    pub patient_id: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyTemplate {
    pub template_id: i64,
}

#[utoipa::path(
    get,
    operation_id = "listTreatments",
    path = "/api/appointments/{id}/treatments",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<Treatment>))
)]
pub async fn list_for_appointment(
    State(state): State<AppState>,
    Path(appointment_id): Path<i64>,
) -> AppResult<Json<Vec<Treatment>>> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM treatment WHERE appointment_id = $1 ORDER BY id",
        appointment_id
    )
    .fetch_all(&state.pool)
    .await?;

    let mut connection = state.pool.acquire().await?;
    let mut treatments = Vec::with_capacity(ids.len());
    for id in ids {
        treatments.push(load(&mut connection, id).await?);
    }
    Ok(Json(treatments))
}

#[utoipa::path(
    post,
    operation_id = "createTreatment",
    path = "/api/appointments/{id}/treatments",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = CreateTreatment,
    responses((status = 200, body = Treatment), (status = 422))
)]
pub async fn create_for_appointment(
    State(state): State<AppState>,
    Path(appointment_id): Path<i64>,
    Json(body): Json<CreateTreatment>,
) -> AppResult<Json<Treatment>> {
    let mut transaction = state.pool.begin().await?;

    // Treatments hang off a *complete* appointment (data-model.md "Draft rows").
    let appointment = sqlx::query!(
        "SELECT draft FROM appointment WHERE id = $1",
        appointment_id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if appointment.draft {
        return Err(AppError::field("appointment_id", "record.incomplete"));
    }

    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO treatment (appointment_id, treatment_reason, finding)
         VALUES ($1, $2, $3) RETURNING id",
        appointment_id,
        body.treatment_reason,
        body.finding,
    )
    .fetch_one(&mut *transaction)
    .await?;

    for patient_id in body.patient_ids {
        attach_patient(&mut transaction, id, patient_id).await?;
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[utoipa::path(
    get,
    operation_id = "getTreatment",
    path = "/api/treatments/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Treatment), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Treatment>> {
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[utoipa::path(
    patch,
    operation_id = "patchTreatment",
    path = "/api/treatments/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = PatchTreatment,
    responses((status = 200, body = Treatment), (status = 404), (status = 409))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchTreatment>,
) -> AppResult<Json<Treatment>> {
    let updated = sqlx::query!(
        r#"UPDATE treatment SET
               treatment_reason = CASE WHEN $2 THEN $3 ELSE treatment_reason END,
               finding          = CASE WHEN $4 THEN $5 ELSE finding END
           WHERE id = $1"#,
        id,
        body.treatment_reason.is_some(),
        body.treatment_reason.flatten(),
        body.finding.is_some(),
        body.finding.flatten(),
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[utoipa::path(
    delete,
    operation_id = "deleteTreatment",
    path = "/api/treatments/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 409))
)]
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let invoiced: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM invoice WHERE treatment_id = $1 AND status <> 'cancelled'
         )",
        id,
    )
    .fetch_one(&state.pool)
    .await?;
    if invoiced.unwrap_or(false) {
        return Err(AppError::Conflict(
            "an invoice exists for this treatment".to_owned(),
        ));
    }

    let deleted = sqlx::query!("DELETE FROM treatment WHERE id = $1", id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    operation_id = "addTreatmentPatient",
    path = "/api/treatments/{id}/patients",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = AddPatient,
    responses((status = 200, body = Treatment), (status = 422))
)]
pub async fn add_patient(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddPatient>,
) -> AppResult<Json<Treatment>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;
    attach_patient(&mut transaction, id, body.patient_id).await?;
    transaction.commit().await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[utoipa::path(
    delete,
    operation_id = "removeTreatmentPatient",
    path = "/api/treatments/{id}/patients/{patient_id}",
    tag = "treatments",
    params(("id" = i64, Path,), ("patient_id" = i64, Path,)),
    responses((status = 200, body = Treatment), (status = 409))
)]
pub async fn remove_patient(
    State(state): State<AppState>,
    Path((id, patient_id)): Path<(i64, i64)>,
) -> AppResult<Json<Treatment>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;

    let attributed: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM treatment_item WHERE treatment_id = $1 AND patient_id = $2
         )",
        id,
        patient_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    if attributed.unwrap_or(false) {
        return Err(AppError::Conflict(
            "the patient is attributed to a line of this treatment".to_owned(),
        ));
    }

    sqlx::query!(
        "DELETE FROM treatment_patient WHERE treatment_id = $1 AND patient_id = $2",
        id,
        patient_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

/// Attaches a patient, enforcing that every patient of a treatment shares one customer
/// (FR-027) — the invoice's customer is derived from that.
async fn attach_patient(
    transaction: &mut Transaction<'_, Postgres>,
    treatment_id: i64,
    patient_id: i64,
) -> AppResult<()> {
    let patient = sqlx::query!(
        "SELECT customer_id, draft FROM patient WHERE id = $1",
        patient_id
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::field("patient_id", "record.notFound"))?;

    if patient.draft {
        return Err(AppError::field("patient_id", "record.incomplete"));
    }
    let customer_id = patient
        .customer_id
        .ok_or_else(|| AppError::field("patient_id", "record.incomplete"))?;

    let existing = sqlx::query_scalar!(
        "SELECT DISTINCT patient.customer_id
         FROM treatment_patient
         JOIN patient ON patient.id = treatment_patient.patient_id
         WHERE treatment_patient.treatment_id = $1",
        treatment_id,
    )
    .fetch_all(&mut **transaction)
    .await?;

    if existing.iter().flatten().any(|other| *other != customer_id) {
        return Err(AppError::field(
            "patient_id",
            "treatment.patientOtherCustomer",
        ));
    }

    sqlx::query!(
        "INSERT INTO treatment_patient (treatment_id, patient_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
        treatment_id,
        patient_id,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[utoipa::path(
    post,
    operation_id = "applyTemplate",
    path = "/api/treatments/{id}/apply-template",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = ApplyTemplate,
    responses((status = 200, body = Vec<TreatmentItem>), (status = 404), (status = 409))
)]
pub async fn apply_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ApplyTemplate>,
) -> AppResult<Json<Vec<TreatmentItem>>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;

    let template = sqlx::query!(
        "SELECT draft FROM treatment_template WHERE id = $1",
        body.template_id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if template.draft {
        return Err(AppError::field("template_id", "record.incomplete"));
    }

    // Template items are appended in template order with current prices copied (FR-024).
    let items = sqlx::query!(
        r#"SELECT kind AS "kind: TreatmentItemKind", drug_packaging_id, service_id, quantity
           FROM treatment_template_item
           WHERE template_id = $1
           ORDER BY position"#,
        body.template_id,
    )
    .fetch_all(&mut *transaction)
    .await?;

    for item in items {
        let reference_id = match item.kind {
            TreatmentItemKind::DrugPackaging => item.drug_packaging_id,
            TreatmentItemKind::Service => item.service_id,
        }
        .ok_or_else(|| AppError::Internal("template item without reference".to_owned()))?;

        treatment_items::insert_pinned_item(
            &mut transaction,
            id,
            item.kind,
            reference_id,
            item.quantity,
            None,
            None,
            None,
            false,
            None,
            &state.config.travel_expenses,
        )
        .await?;
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        treatment_items::load_items(&mut connection, id).await?,
    ))
}

#[utoipa::path(
    post,
    operation_id = "duplicateTreatment",
    path = "/api/treatments/{id}/duplicate",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = DuplicateRequest,
    responses((status = 200, body = Treatment), (status = 404))
)]
pub async fn duplicate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<DuplicateRequest>,
) -> AppResult<Json<Treatment>> {
    let mut transaction = state.pool.begin().await?;
    let appointment_id: i64 =
        sqlx::query_scalar!("SELECT appointment_id FROM treatment WHERE id = $1", id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::NotFound)?;

    let new_id = copy_treatment(
        &mut transaction,
        id,
        appointment_id,
        body.price_mode,
        &state.config.travel_expenses,
    )
    .await?;
    transaction.commit().await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, new_id).await?))
}

/// Copies a treatment (reason, finding, patients, lines) into `target_appointment_id`.
///
/// `PriceMode::Verbatim` keeps the pinned values, `PriceMode::Refresh` re-reads names and
/// prices from the catalog (FR-026).
pub async fn copy_treatment(
    transaction: &mut Transaction<'_, Postgres>,
    source_id: i64,
    target_appointment_id: i64,
    price_mode: PriceMode,
    travel: &TravelExpenseConfig,
) -> AppResult<i64> {
    let source = sqlx::query!(
        "SELECT treatment_reason, finding FROM treatment WHERE id = $1",
        source_id
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let new_id: i64 = sqlx::query_scalar!(
        "INSERT INTO treatment (appointment_id, treatment_reason, finding)
         VALUES ($1, $2, $3) RETURNING id",
        target_appointment_id,
        source.treatment_reason,
        source.finding,
    )
    .fetch_one(&mut **transaction)
    .await?;

    sqlx::query!(
        "INSERT INTO treatment_patient (treatment_id, patient_id)
         SELECT $1, patient_id FROM treatment_patient WHERE treatment_id = $2",
        new_id,
        source_id,
    )
    .execute(&mut **transaction)
    .await?;

    let items = sqlx::query!(
        r#"SELECT kind AS "kind: TreatmentItemKind", drug_packaging_id, service_id, patient_id,
                  name, quantity, unit, factor, got_number, price_net, vat_percent,
                  km, km_multiplier, redesignation
           FROM treatment_item WHERE treatment_id = $1 ORDER BY position"#,
        source_id,
    )
    .fetch_all(&mut **transaction)
    .await?;

    for item in items {
        let reference_id = match item.kind {
            TreatmentItemKind::DrugPackaging => item.drug_packaging_id,
            TreatmentItemKind::Service => item.service_id,
        }
        .ok_or_else(|| AppError::Internal("treatment item without reference".to_owned()))?;

        let pinned = match price_mode {
            PriceMode::Verbatim => Some(CatalogLine {
                name: item.name,
                price_net: item.price_net,
                vat_percent: item.vat_percent,
                unit: item.unit,
                factor: item.factor,
                got_number: item.got_number,
                travel_expenses: item.km.is_some(),
            }),
            PriceMode::Refresh => None,
        };

        treatment_items::insert_pinned_item(
            transaction,
            new_id,
            item.kind,
            reference_id,
            item.quantity,
            item.patient_id,
            item.km,
            item.km_multiplier,
            item.redesignation,
            pinned,
            travel,
        )
        .await?;
    }
    Ok(new_id)
}

/// Loads a treatment with its patients, live invoice and total.
pub async fn load(connection: &mut PgConnection, id: i64) -> AppResult<Treatment> {
    let row = sqlx::query!(
        r#"SELECT treatment.id, treatment.appointment_id, treatment.treatment_reason,
                  treatment.finding, treatment.created_at, treatment.updated_at,
                  appointment.starts_at
           FROM treatment
           JOIN appointment ON appointment.id = treatment.appointment_id
           WHERE treatment.id = $1"#,
        id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let patients = sqlx::query!(
        r#"SELECT patient.id, patient.name, patient.customer_id, patient.warning_remark
           FROM treatment_patient
           JOIN patient ON patient.id = treatment_patient.patient_id
           WHERE treatment_patient.treatment_id = $1
           ORDER BY patient.name"#,
        id,
    )
    .fetch_all(&mut *connection)
    .await?;

    // The live invoice wins; without one the latest cancelled invoice is shown so the
    // treatment page never looks as if nothing had happened.
    let invoice = sqlx::query!(
        r#"SELECT id, invoice_number, status AS "status: InvoiceStatus"
           FROM invoice
           WHERE treatment_id = $1
           ORDER BY (status <> 'cancelled') DESC, ts_cancelled DESC NULLS LAST, id DESC
           LIMIT 1"#,
        id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .map(|row| TreatmentInvoice {
        id: row.id,
        invoice_number: row.invoice_number,
        status: row.status,
    });

    let items = treatment_items::load_items(&mut *connection, id).await?;
    let groups = money::vat_summary(
        &items
            .iter()
            .map(|item| (item.line_net, item.vat_percent))
            .collect::<Vec<_>>(),
    );

    let customer_id = patients.first().and_then(|patient| patient.customer_id);
    let customer_emails = match customer_id {
        Some(customer_id) => {
            sqlx::query_scalar!(
                "SELECT email FROM customer_email WHERE customer_id = $1 ORDER BY id",
                customer_id,
            )
            .fetch_all(&mut *connection)
            .await?
        }
        None => Vec::new(),
    };
    let frozen = stock::treatment_is_frozen(&mut *connection, id).await?;

    Ok(Treatment {
        id: row.id,
        appointment_id: row.appointment_id,
        starts_at: row.starts_at,
        treatment_reason: row.treatment_reason,
        finding: row.finding,
        patients: patients
            .into_iter()
            .map(|patient| TreatmentPatient {
                patient_id: patient.id,
                name: patient.name.unwrap_or_default(),
                customer_id: patient.customer_id.unwrap_or_default(),
                warning_remark: patient.warning_remark,
            })
            .collect(),
        customer_id,
        customer_emails,
        invoice,
        frozen,
        total_gross: money::total_gross(&groups),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/treatments/{id}", get(detail).patch(patch).delete(delete))
        .route("/treatments/{id}/patients", post(add_patient))
        .route(
            "/treatments/{id}/patients/{patient_id}",
            axum::routing::delete(remove_patient),
        )
        .route(
            "/treatments/{id}/items",
            get(treatment_items::list).post(treatment_items::create),
        )
        .route("/treatments/{id}/apply-template", post(apply_template))
        .route("/treatments/{id}/duplicate", post(duplicate))
}
