//! Patientenbehandlungen — what was done to one animal during one visit.
//!
//! A treatment covers the visit and carries the invoice; this carries the clinical record of
//! a single animal: why it was seen, what was found, its files, and the positions billed for
//! it. Positions belong here rather than to the treatment because a dispense movement has no
//! patient of its own — which animal received which batch is derived from the line's owner.

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::double_option;
use crate::api::patients::PatientFile;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct PatientTreatment {
    pub id: i64,
    pub treatment_id: i64,
    pub patient_id: i64,
    pub patient_name: Option<String>,
    /// The visit's date, for the page's heading — the record has no date of its own.
    pub starts_at: Option<DateTime<Utc>>,
    pub treatment_reason: Option<String>,
    pub finding: Option<String>,
    /// `true` once the treatment's invoice is accepted: the record reads read-only.
    pub frozen: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchPatientTreatment {
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub treatment_reason: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub finding: Option<Option<String>>,
}

#[utoipa::path(
    get,
    operation_id = "getPatientTreatment",
    path = "/api/patient-treatments/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = PatientTreatment), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<PatientTreatment>> {
    Ok(Json(load(&state, id).await?))
}

#[utoipa::path(
    patch,
    operation_id = "patchPatientTreatment",
    path = "/api/patient-treatments/{id}",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = PatchPatientTreatment,
    responses((status = 200, body = PatientTreatment), (status = 404))
)]
pub async fn patch_record(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPatientTreatment>,
) -> AppResult<Json<PatientTreatment>> {
    // The reason and the finding stay editable on a frozen treatment, as they were when they
    // lived on it: an accepted invoice freezes what is billed, not what was observed.
    let updated = sqlx::query!(
        r#"UPDATE patient_treatment SET
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
    Ok(Json(load(&state, id).await?))
}

#[utoipa::path(
    get,
    operation_id = "listPatientTreatmentFiles",
    path = "/api/patient-treatments/{id}/files",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<PatientFile>))
)]
pub async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<PatientFile>>> {
    // What was brought or produced for this animal during this visit — a lab result, a
    // photograph of the wound. The patient's own files are the ones that outlive the visit.
    let files = sqlx::query_as!(
        PatientFile,
        r#"SELECT id, orig_name, mime_type, size_bytes, reference_date, note, has_thumbnail,
                  created_at
           FROM attachment
           WHERE patient_treatment_id = $1 AND kind = 'treatment_file'
           ORDER BY COALESCE(reference_date, created_at::date) DESC, id DESC"#,
        id,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(files))
}

async fn load(state: &AppState, id: i64) -> AppResult<PatientTreatment> {
    let row = sqlx::query!(
        r#"SELECT record.id, record.treatment_id, record.patient_id, record.treatment_reason,
                  record.finding, record.created_at, record.updated_at,
                  patient.name AS "patient_name?",
                  appointment.starts_at,
                  EXISTS (
                      SELECT 1 FROM invoice
                      WHERE invoice.treatment_id = record.treatment_id
                        AND invoice.status IN ('accepted', 'sent', 'submitted')
                  ) AS "frozen?"
           FROM patient_treatment record
           JOIN patient ON patient.id = record.patient_id
           JOIN treatment ON treatment.id = record.treatment_id
           JOIN appointment ON appointment.id = treatment.appointment_id
           WHERE record.id = $1"#,
        id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(PatientTreatment {
        id: row.id,
        treatment_id: row.treatment_id,
        patient_id: row.patient_id,
        patient_name: row.patient_name,
        starts_at: row.starts_at,
        treatment_reason: row.treatment_reason,
        finding: row.finding,
        frozen: row.frozen.unwrap_or(false),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/patient-treatments/{id}", get(detail))
        .route("/patient-treatments/{id}", patch(patch_record))
        .route("/patient-treatments/{id}/files", get(list_files))
}
