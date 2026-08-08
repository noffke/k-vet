//! Patients: the practice's animal index, with photo, files and warnings (T044).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::domain::draft::{missing_fields, recompute_draft};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Patient {
    pub id: i64,
    pub customer_id: Option<i64>,
    /// Customer name, so lists and pickers can show the owner.
    pub customer_name: Option<String>,
    pub name: Option<String>,
    /// `female`, `male` or `unknown`.
    pub sex: Option<String>,
    pub species: Option<String>,
    pub race: Option<String>,
    pub colour: Option<String>,
    /// Weight in kilograms, one decimal. A single current value: each weighing replaces the
    /// last, so there is no history.
    pub weight_kg: Option<Decimal>,
    pub date_of_birth: Option<NaiveDate>,
    pub photo_attachment_id: Option<i64>,
    pub date_of_death: Option<NaiveDate>,
    pub chip_number: Option<String>,
    pub eu_passport_number: Option<String>,
    pub warning_remark: Option<String>,
    pub neutered: bool,
    pub insured: bool,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreatePatient {
    /// Pre-selects the owner when the patient is created from a customer page.
    pub customer_id: Option<i64>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchPatient {
    #[serde(default, deserialize_with = "double_option")]
    pub customer_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub sex: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub species: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub race: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub colour: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub weight_kg: Option<Option<Decimal>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_of_birth: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub photo_attachment_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_of_death: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub chip_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub eu_passport_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub warning_remark: Option<Option<String>>,
    pub neutered: Option<bool>,
    pub insured: Option<bool>,
}

/// Files a customer brought along, kept per patient with the date they refer to.
#[derive(Debug, Serialize, ToSchema)]
pub struct PatientFile {
    pub id: i64,
    pub orig_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub reference_date: Option<NaiveDate>,
    pub note: Option<String>,
    pub has_thumbnail: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchPatientFile {
    #[serde(default, deserialize_with = "double_option")]
    pub reference_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
}

#[utoipa::path(
    get,
    path = "/api/patients",
    operation_id = "listPatients",
    tag = "patients",
    params(ListQuery, ("customer_id" = Option<i64>, Query, description = "Only this customer's animals")),
    responses((status = 200, body = Vec<Patient>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    Query(filter): Query<PatientFilter>,
) -> AppResult<Json<Vec<Patient>>> {
    let rows = sqlx::query!(
        r#"SELECT patient.id
           FROM patient
           LEFT JOIN customer ON customer.id = patient.customer_id
           WHERE ($2 OR NOT patient.archived)
             AND ($3::bigint IS NULL OR patient.customer_id = $3)
             AND ($1::text IS NULL
                  OR patient.name ILIKE '%' || $1 || '%'
                  OR patient.chip_number ILIKE '%' || $1 || '%'
                  OR customer.last_name ILIKE '%' || $1 || '%')
           ORDER BY patient.name NULLS LAST, patient.id
           LIMIT $4 OFFSET $5"#,
        query.search(),
        query.include_archived(),
        filter.customer_id,
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    let mut patients = Vec::with_capacity(rows.len());
    for row in rows {
        patients.push(load(&state.pool, row.id).await?);
    }
    Ok(Json(patients))
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PatientFilter {
    pub customer_id: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/patients",
    operation_id = "createPatient",
    tag = "patients",
    request_body = CreatePatient,
    responses((status = 200, description = "An empty draft, ready for auto-save", body = Patient))
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreatePatient>,
) -> AppResult<Json<Patient>> {
    let id: i64 = sqlx::query_scalar!(
        "INSERT INTO patient (customer_id) VALUES ($1) RETURNING id",
        body.customer_id,
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/patients/{id}",
    operation_id = "getPatient",
    tag = "patients",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Patient), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Patient>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/patients/{id}",
    operation_id = "patchPatient",
    tag = "patients",
    params(("id" = i64, Path,)),
    request_body = PatchPatient,
    responses((status = 200, body = Patient), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPatient>,
) -> AppResult<Json<Patient>> {
    // The column is NUMERIC(5,1), so Postgres would silently round a second decimal away. The
    // form never sends one — it rounds as the vet types, like every other number field — so this
    // guards the API against a client that would otherwise have its value quietly changed.
    if let Some(Some(weight)) = body.weight_kg {
        if weight.scale() > 1 {
            return Err(AppError::field("weight_kg", "value.tooPrecise"));
        }
        if weight <= Decimal::ZERO {
            return Err(AppError::field("weight_kg", "value.mustBePositive"));
        }
    }

    let mut transaction = state.pool.begin().await?;

    let current = sqlx::query!(
        "SELECT customer_id, name, sex, species, date_of_death, archived, draft
         FROM patient WHERE id = $1 FOR UPDATE",
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(Some(sex)) = &body.sex
        && !["female", "male", "unknown"].contains(&sex.as_str())
    {
        return Err(AppError::field("sex", "patient.sex.invalid"));
    }

    let text_present = |patched: &Option<Option<String>>, stored: &Option<String>| match patched {
        Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
        None => stored.is_some(),
    };
    let customer_present = match &body.customer_id {
        Some(value) => value.is_some(),
        None => current.customer_id.is_some(),
    };
    let draft = recompute_draft(
        current.draft,
        &[
            ("customer_id", customer_present),
            ("name", text_present(&body.name, &current.name)),
            ("sex", text_present(&body.sex, &current.sex)),
            ("species", text_present(&body.species, &current.species)),
        ],
    )?;

    // A recorded date of death archives the patient (FR-009).
    let date_of_death = match body.date_of_death {
        Some(value) => value,
        None => current.date_of_death,
    };
    let archived = current.archived || date_of_death.is_some();

    sqlx::query!(
        r#"UPDATE patient SET
               customer_id         = CASE WHEN $2  THEN $3  ELSE customer_id END,
               name                = CASE WHEN $4  THEN $5  ELSE name END,
               sex                 = CASE WHEN $6  THEN $7  ELSE sex END,
               species             = CASE WHEN $8  THEN $9  ELSE species END,
               race                = CASE WHEN $10 THEN $11 ELSE race END,
               colour              = CASE WHEN $12 THEN $13 ELSE colour END,
               date_of_birth       = CASE WHEN $14 THEN $15 ELSE date_of_birth END,
               photo_attachment_id = CASE WHEN $16 THEN $17 ELSE photo_attachment_id END,
               date_of_death       = CASE WHEN $18 THEN $19 ELSE date_of_death END,
               chip_number         = CASE WHEN $20 THEN $21 ELSE chip_number END,
               eu_passport_number  = CASE WHEN $22 THEN $23 ELSE eu_passport_number END,
               warning_remark      = CASE WHEN $24 THEN $25 ELSE warning_remark END,
               weight_kg           = CASE WHEN $26 THEN $27 ELSE weight_kg END,
               neutered            = COALESCE($28, neutered),
               insured             = COALESCE($29, insured),
               archived            = $30,
               draft               = $31
           WHERE id = $1"#,
        id,
        body.customer_id.is_some(),
        body.customer_id.flatten(),
        body.name.is_some(),
        blank_to_null(&body.name),
        body.sex.is_some(),
        blank_to_null(&body.sex),
        body.species.is_some(),
        blank_to_null(&body.species),
        body.race.is_some(),
        blank_to_null(&body.race),
        body.colour.is_some(),
        blank_to_null(&body.colour),
        body.date_of_birth.is_some(),
        body.date_of_birth.flatten(),
        body.photo_attachment_id.is_some(),
        body.photo_attachment_id.flatten(),
        body.date_of_death.is_some(),
        body.date_of_death.flatten(),
        body.chip_number.is_some(),
        blank_to_null(&body.chip_number),
        body.eu_passport_number.is_some(),
        blank_to_null(&body.eu_passport_number),
        body.warning_remark.is_some(),
        blank_to_null(&body.warning_remark),
        body.weight_kg.is_some(),
        body.weight_kg.flatten(),
        body.neutered,
        body.insured,
        archived,
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

fn blank_to_null(value: &Option<Option<String>>) -> Option<String> {
    value
        .clone()
        .flatten()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

#[utoipa::path(
    post,
    path = "/api/patients/{id}/archive",
    operation_id = "archivePatient",
    tag = "patients",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Patient))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Patient>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/patients/{id}/unarchive",
    operation_id = "unarchivePatient",
    tag = "patients",
    params(("id" = i64, Path,)),
    responses(
        (status = 200, body = Patient),
        (status = 409, description = "A deceased patient stays archived")
    )
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Patient>> {
    let deceased: Option<NaiveDate> =
        sqlx::query_scalar!("SELECT date_of_death FROM patient WHERE id = $1", id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?;
    if deceased.is_some() {
        return Err(AppError::Conflict(
            "a patient with a date of death stays archived".to_owned(),
        ));
    }
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<Patient>> {
    let updated = sqlx::query!(
        "UPDATE patient SET archived = $2 WHERE id = $1",
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
    path = "/api/patients/{id}/files",
    operation_id = "listPatientFiles",
    tag = "patients",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Vec<PatientFile>))
)]
pub async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<PatientFile>>> {
    let files = sqlx::query_as!(
        PatientFile,
        r#"SELECT id, orig_name, mime_type, size_bytes, reference_date, note, has_thumbnail,
                  created_at
           FROM attachment
           WHERE patient_id = $1 AND kind = 'patient_file'
           ORDER BY COALESCE(reference_date, created_at::date) DESC, id DESC"#,
        id,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(files))
}

#[utoipa::path(
    patch,
    path = "/api/patient-files/{id}",
    operation_id = "patchPatientFile",
    tag = "patients",
    params(("id" = i64, Path,)),
    request_body = PatchPatientFile,
    responses((status = 200, body = PatientFile), (status = 404))
)]
pub async fn patch_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPatientFile>,
) -> AppResult<Json<PatientFile>> {
    let file = sqlx::query_as!(
        PatientFile,
        r#"UPDATE attachment SET
               reference_date = CASE WHEN $2 THEN $3 ELSE reference_date END,
               note           = CASE WHEN $4 THEN $5 ELSE note END
           WHERE id = $1 AND kind = 'patient_file'
           RETURNING id, orig_name, mime_type, size_bytes, reference_date, note, has_thumbnail,
                     created_at"#,
        id,
        body.reference_date.is_some(),
        body.reference_date.flatten(),
        body.note.is_some(),
        body.note.clone().flatten(),
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(file))
}

pub async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Patient> {
    let row = sqlx::query!(
        r#"SELECT patient.id, patient.customer_id, patient.name, patient.sex, patient.species,
                  patient.race, patient.colour, patient.weight_kg, patient.date_of_birth,
                  patient.photo_attachment_id, patient.date_of_death, patient.chip_number,
                  patient.eu_passport_number, patient.warning_remark, patient.neutered,
                  patient.insured, patient.archived, patient.draft, patient.created_at,
                  patient.updated_at,
                  customer.first_name AS "customer_first_name?",
                  customer.last_name AS "customer_last_name?"
           FROM patient
           LEFT JOIN customer ON customer.id = patient.customer_id
           WHERE patient.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let customer_name = match (&row.customer_first_name, &row.customer_last_name) {
        (None, None) => None,
        (first, last) => Some(
            [first.clone(), last.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
        ),
    };

    let missing_fields = missing_fields(&[
        ("customer_id", row.customer_id.is_some()),
        ("name", row.name.is_some()),
        ("sex", row.sex.is_some()),
        ("species", row.species.is_some()),
    ])
    .into_iter()
    .map(str::to_owned)
    .collect();

    Ok(Patient {
        id: row.id,
        customer_id: row.customer_id,
        customer_name,
        name: row.name,
        sex: row.sex,
        species: row.species,
        race: row.race,
        colour: row.colour,
        weight_kg: row.weight_kg,
        date_of_birth: row.date_of_birth,
        photo_attachment_id: row.photo_attachment_id,
        date_of_death: row.date_of_death,
        chip_number: row.chip_number,
        eu_passport_number: row.eu_passport_number,
        warning_remark: row.warning_remark,
        neutered: row.neutered,
        insured: row.insured,
        archived: row.archived,
        draft: row.draft,
        missing_fields,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RaceFilter {
    /// Only the races recorded for this Tierart; absent means all of them.
    pub species: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/patients/races",
    operation_id = "listRaces",
    tag = "patients",
    params(RaceFilter),
    responses((status = 200, description = "Races already recorded, alphabetical", body = Vec<String>))
)]
pub async fn races(
    State(state): State<AppState>,
    Query(filter): Query<RaceFilter>,
) -> AppResult<Json<Vec<String>>> {
    // An editable select of what the practice has already seen. Scoped by Tierart, because
    // the breeds of a dog are no help while entering a rabbit.
    let species = filter
        .species
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let races = sqlx::query_scalar!(
        "SELECT DISTINCT race FROM patient
          WHERE race IS NOT NULL AND race <> ''
            AND ($1::text IS NULL OR lower(species) = lower($1))
          ORDER BY race",
        species,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(races.into_iter().flatten().collect()))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/patients/races", get(races))
        .route("/patients", get(list).post(create))
        .route("/patients/{id}", get(detail).patch(patch))
        .route("/patients/{id}/archive", post(archive))
        .route("/patients/{id}/unarchive", post(unarchive))
        .route("/patients/{id}/files", get(list_files))
        .route("/patient-files/{id}", axum::routing::patch(patch_file))
}
