//! Treatments — the medical record of one visit, and the source of every invoice (T031).
//!
//! Every appointment has exactly one, made with it (migration 0022); there is no endpoint to
//! create, list or delete them separately.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as Jsonb;
use sqlx::{PgConnection, Postgres, Transaction};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::appointments::{self, Appointment};
use crate::api::attachments;
use crate::api::common::PriceMode;
use crate::api::travel_distance::{self, KmSource};
use crate::api::treatment_items::{self, Attribution, CatalogLine, TreatmentItem};
use crate::config::TravelExpenseConfig;
use crate::domain::enums::{InvoiceStatus, TreatmentItemKind};
use crate::domain::money;
use crate::domain::rich_text::RichText;
use crate::domain::stock;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Treatment {
    pub id: i64,
    pub appointment_id: i64,
    pub starts_at: Option<DateTime<Utc>>,
    /// One record per animal, each with its own reason, finding and positions.
    pub patients: Vec<TreatmentPatient>,
    /// Whose visit this is, inherited from the appointment — the invoice's customer, and what
    /// limits which animals may be attached (FR-027).
    pub customer_id: Option<i64>,
    /// The customer's email addresses, offered as invoice recipients (FR-031).
    pub customer_emails: Vec<String>,
    /// The live invoice of this treatment, or the most recent cancelled one — the vet has
    /// to see that the last attempt was cancelled, and can then bill again.
    pub invoice: Option<TreatmentInvoice>,
    /// How many positions are billed here. Zero means there is nothing to invoice yet, which
    /// is what tells "still being written up" apart from "worked and never billed" (FR-036).
    pub item_count: i64,
    /// `true` once the invoice is accepted: lines and stock movements are frozen.
    pub frozen: bool,
    /// The treatment changed after its invoice PDF was rendered, so the document on file no
    /// longer shows what is billed. The PDF is only ever written when an invoice is created.
    pub pdf_stale: bool,
    pub total_gross: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentPatient {
    /// The Patientenbehandlung — what positions and files hang on.
    pub id: i64,
    pub patient_id: i64,
    pub name: String,
    pub customer_id: i64,
    pub warning_remark: Option<String>,
    pub treatment_reason: Option<RichText>,
    pub finding: Option<RichText>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentInvoice {
    pub id: i64,
    pub invoice_number: String,
    pub status: InvoiceStatus,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddPatient {
    pub patient_id: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyTemplate {
    pub template_id: i64,
    /// The animal the group's lines are for. Absent means the treatment's only animal, and
    /// with several it means none of them — which its drug lines will refuse.
    pub patient_treatment_id: Option<i64>,
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
    ensure_open(&mut transaction, id).await?;
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
    responses(
        (status = 200, body = Treatment),
        (status = 409, description = "The treatment's invoice is accepted"),
        (status = 422, description = "The customer's only animal")
    )
)]
pub async fn remove_patient(
    State(state): State<AppState>,
    Path((id, patient_id)): Path<(i64, i64)>,
) -> AppResult<Json<Treatment>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;

    // The customer's only animal is the treatment's, and stays on it (issues.md, 2026-10-17):
    // there is nothing else it could be. With a second animal it can come off again.
    if eligible_animals(&mut transaction, id).await? == [patient_id] {
        return Err(AppError::field("patient_id", "treatment.onlyPatient"));
    }

    // Everything written for the animal on this visit goes with it, asked first in the app:
    // its lines — as removing them by hand would, so draft dispenses put the drugs back on the
    // shelf — its texts, and its files.
    let mut hashes = Vec::new();
    if let Some(record_id) = record_of(&mut transaction, id, patient_id).await? {
        let lines = sqlx::query_scalar!(
            "SELECT id FROM treatment_item WHERE patient_treatment_id = $1",
            record_id,
        )
        .fetch_all(&mut *transaction)
        .await?;
        treatment_items::delete_lines(&mut transaction, id, &lines).await?;
        hashes = sqlx::query_scalar!(
            "SELECT sha256 FROM attachment WHERE patient_treatment_id = $1",
            record_id,
        )
        .fetch_all(&mut *transaction)
        .await?;
        // The file rows go with the record (ON DELETE CASCADE).
        sqlx::query!("DELETE FROM patient_treatment WHERE id = $1", record_id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    // Only once the rows are gone for good; bytes another row still shares stay.
    attachments::remove_unused_files(&state.pool, &state.files, &hashes).await?;

    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

/// Moves an animal to an appointment of its own — same customer, same time — with its record:
/// its texts, its lines with their draft dispenses, its files. The visit's own lines (house
/// call, Wegegeld) stay where they are. For the second pet the customer wants on another
/// invoice (issues.md).
#[utoipa::path(
    post,
    operation_id = "moveAnimalToOwnAppointment",
    path = "/api/treatments/{id}/patients/{patient_id}/own-appointment",
    tag = "treatments",
    params(("id" = i64, Path,), ("patient_id" = i64, Path,)),
    responses(
        (status = 200, description = "The animal's new appointment", body = Appointment),
        (status = 404, description = "The animal is not on this treatment"),
        (status = 409, description = "The treatment's invoice is accepted"),
        (status = 422, description = "The animal is the visit's only one")
    )
)]
pub async fn move_to_own_appointment(
    State(state): State<AppState>,
    Path((id, patient_id)): Path<(i64, i64)>,
) -> AppResult<Json<Appointment>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;
    ensure_open(&mut transaction, id).await?;
    let record_id = record_of(&mut transaction, id, patient_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let animals = sqlx::query_scalar!(
        "SELECT count(*) FROM patient_treatment WHERE treatment_id = $1",
        id,
    )
    .fetch_one(&mut *transaction)
    .await?
    .unwrap_or(0);
    if animals < 2 {
        return Err(AppError::field("patient_id", "treatment.aloneAlready"));
    }

    let visit = sqlx::query!(
        "SELECT appointment.starts_at, appointment.customer_id
           FROM treatment JOIN appointment ON appointment.id = treatment.appointment_id
          WHERE treatment.id = $1",
        id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    let appointment_id: i64 = sqlx::query_scalar!(
        "INSERT INTO appointment (starts_at, customer_id, draft)
         VALUES ($1, $2, false) RETURNING id",
        visit.starts_at,
        visit.customer_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    let treatment_id: i64 = sqlx::query_scalar!(
        "INSERT INTO treatment (appointment_id, customer_id) VALUES ($1, $2) RETURNING id",
        appointment_id,
        visit.customer_id,
    )
    .fetch_one(&mut *transaction)
    .await?;
    // The lines follow the record by the foreign key's ON UPDATE CASCADE (migration 0033); the
    // dispenses and files hang on the lines and the record, so they come along unasked.
    sqlx::query!(
        "UPDATE patient_treatment SET treatment_id = $1 WHERE id = $2",
        treatment_id,
        record_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(appointments::load(&state.pool, appointment_id).await?))
}

/// The record of `patient_id` on the treatment, if the animal is on it.
async fn record_of(
    connection: &mut PgConnection,
    treatment_id: i64,
    patient_id: i64,
) -> AppResult<Option<i64>> {
    Ok(sqlx::query_scalar!(
        "SELECT id FROM patient_treatment WHERE treatment_id = $1 AND patient_id = $2",
        treatment_id,
        patient_id,
    )
    .fetch_optional(&mut *connection)
    .await?)
}

/// Refuses content on a treatment whose appointment is not ready for it.
///
/// The treatment exists from the moment the appointment does, a draft without a time or a
/// customer included. Animals and lines wait until both are there: what is billed has to have a
/// date, and whose visit it is decides which animals are on offer (issues.md 7). This is the
/// gate the old "add treatment" step used to be.
pub async fn ensure_open(connection: &mut PgConnection, treatment_id: i64) -> AppResult<()> {
    let row = sqlx::query!(
        "SELECT appointment.draft, treatment.customer_id
           FROM treatment
           JOIN appointment ON appointment.id = treatment.appointment_id
          WHERE treatment.id = $1",
        treatment_id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.draft {
        return Err(AppError::field("appointment_id", "record.incomplete"));
    }
    if row.customer_id.is_none() {
        return Err(AppError::field("customer_id", "treatment.customerRequired"));
    }
    Ok(())
}

/// The animals a treatment's customer could bring: complete, not archived, alive. At most two
/// are read — every caller only asks "none, exactly one, or more".
async fn eligible_animals(connection: &mut PgConnection, treatment_id: i64) -> AppResult<Vec<i64>> {
    Ok(sqlx::query_scalar!(
        "SELECT patient.id
           FROM patient
           JOIN treatment ON treatment.customer_id = patient.customer_id
          WHERE treatment.id = $1
            AND NOT patient.draft AND NOT patient.archived AND patient.date_of_death IS NULL
          ORDER BY patient.id
          LIMIT 2",
        treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?)
}

/// Puts the customer's only animal on a treatment that has none yet (issues.md, 2026-10-17):
/// with one animal there is nothing to choose, so the picker has nothing to ask.
pub async fn attach_only_animal(
    transaction: &mut Transaction<'_, Postgres>,
    treatment_id: i64,
) -> AppResult<()> {
    let attached: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM patient_treatment WHERE treatment_id = $1)",
        treatment_id,
    )
    .fetch_one(&mut **transaction)
    .await?;
    if attached.unwrap_or(false) {
        return Ok(());
    }
    if let [only] = eligible_animals(transaction, treatment_id).await?[..] {
        attach_patient(transaction, treatment_id, only).await?;
    }
    Ok(())
}

/// Whether anything has been entered on a treatment beyond the bare animals: a line, an
/// invoice, a reason or finding, a file. Until then the visit can still move to another
/// customer — its animals are only the default, and go with the change.
pub async fn has_content(connection: &mut PgConnection, treatment_id: i64) -> AppResult<bool> {
    let content: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM treatment_item WHERE treatment_id = $1)
             OR EXISTS (SELECT 1 FROM invoice WHERE treatment_id = $1)
             OR EXISTS (SELECT 1 FROM patient_treatment
                         WHERE treatment_id = $1
                           AND (treatment_reason IS NOT NULL OR finding IS NOT NULL))
             OR EXISTS (SELECT 1 FROM attachment
                          JOIN patient_treatment ON patient_treatment.id = attachment.patient_treatment_id
                         WHERE patient_treatment.treatment_id = $1)",
        treatment_id,
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(content.unwrap_or(false))
}

/// Attaches a patient to a treatment.
///
/// That every animal on a treatment belongs to one customer (FR-027) is now the database's
/// job: `patient_treatment` carries the customer and composite foreign keys tie it to both the
/// treatment's and the animal's. What is left here is turning the resulting constraint error
/// into something the screen can say, and refusing a draft animal, which no key can express.
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

    // Read the treatment's own customer rather than the first animal already on it: an empty
    // treatment has no animal to read, and that gap is what let the picker offer the whole
    // practice until the first one was attached.
    let treatment_customer: Option<i64> = sqlx::query_scalar!(
        "SELECT customer_id FROM treatment WHERE id = $1",
        treatment_id
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if treatment_customer.is_some_and(|owner| owner != customer_id) {
        return Err(AppError::field(
            "patient_id",
            "treatment.patientOtherCustomer",
        ));
    }

    sqlx::query!(
        "INSERT INTO patient_treatment (treatment_id, patient_id, customer_id)
         VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
        treatment_id,
        patient_id,
        customer_id,
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
    // A group with a Wegegeld line gets its kilometres looked up once, before the transaction,
    // as adding the line by hand does; the other lines take none.
    let has_travel = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM treatment_template_item item
               JOIN service ON service.id = item.service_id
               WHERE item.template_id = $1 AND service.travel_expenses
           ) AS "exists!""#,
        body.template_id,
    )
    .fetch_one(&state.pool)
    .await?;
    let suggestion = if has_travel {
        let mut connection = state.pool.acquire().await?;
        stock::ensure_editable(&mut connection, id).await?;
        ensure_open(&mut connection, id).await?;
        drop(connection);
        travel_distance::suggest_km(&state, id).await?
    } else {
        None
    };
    let km = suggestion.as_ref().and_then(|suggestion| suggestion.km);

    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;
    ensure_open(&mut transaction, id).await?;

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
        r#"SELECT item.kind AS "kind: TreatmentItemKind", item.drug_packaging_id,
                  item.service_id, item.quantity,
                  COALESCE(service.travel_expenses, false) AS "travel!"
           FROM treatment_template_item item
           LEFT JOIN service ON service.id = item.service_id
           WHERE item.template_id = $1
           ORDER BY item.position"#,
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
            body.patient_treatment_id
                .map_or(Attribution::Default, Attribution::Record),
            // The kilometres go on the Wegegeld line only: a line stores what it is given.
            if item.travel { km } else { None },
            None,
            false,
            None,
            &state.config.travel_expenses,
        )
        .await?;
    }
    if suggestion.is_some_and(|suggestion| suggestion.source == Some(KmSource::Route)) {
        travel_distance::clear_travel_mark(&mut transaction, id).await?;
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        treatment_items::load_items(&mut connection, id).await?,
    ))
}

/// Copies a treatment (reason, finding, patients, lines) into `target_appointment_id`, which
/// must not have its treatment yet, as that appointment's treatment for `customer_id`.
///
/// `PriceMode::Verbatim` keeps the pinned values, `PriceMode::Refresh` re-reads names and
/// prices from the catalog (FR-026).
pub async fn copy_treatment(
    transaction: &mut Transaction<'_, Postgres>,
    source_id: i64,
    target_appointment_id: i64,
    customer_id: Option<i64>,
    price_mode: PriceMode,
    travel: &TravelExpenseConfig,
) -> AppResult<i64> {
    let exists: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM treatment WHERE id = $1)",
        source_id
    )
    .fetch_one(&mut **transaction)
    .await?;
    if !exists.unwrap_or(false) {
        return Err(AppError::NotFound);
    }

    // The customer travels with the copy. It used to be left out, and the composite keys skip
    // NULLs, so a copy was accepted that could never be billed and offered every animal in the
    // practice.
    let new_id: i64 = sqlx::query_scalar!(
        "INSERT INTO treatment (appointment_id, customer_id) VALUES ($1, $2) RETURNING id",
        target_appointment_id,
        customer_id,
    )
    .fetch_one(&mut **transaction)
    .await?;

    // Each animal's record is copied with its reason and finding, and the copy's lines have
    // to point at the copy's records — so the new ids are kept by animal.
    let copied = sqlx::query!(
        "INSERT INTO patient_treatment
             (treatment_id, patient_id, customer_id, treatment_reason, finding)
         SELECT $1, patient_id, customer_id, treatment_reason, finding
           FROM patient_treatment WHERE treatment_id = $2
         RETURNING id, patient_id",
        new_id,
        source_id,
    )
    .fetch_all(&mut **transaction)
    .await?;
    let records: HashMap<i64, i64> = copied
        .into_iter()
        .map(|row| (row.patient_id, row.id))
        .collect();

    let items = sqlx::query!(
        r#"SELECT item.kind AS "kind: TreatmentItemKind", item.drug_packaging_id,
                  item.service_id, record.patient_id AS "patient_id?",
                  item.name, item.quantity, item.unit, item.factor, item.got_number,
                  item.price_net, item.vat_percent, item.km, item.km_multiplier,
                  item.redesignation, item.per_visit
           FROM treatment_item item
           LEFT JOIN patient_treatment record ON record.id = item.patient_treatment_id
           WHERE item.treatment_id = $1 ORDER BY item.position"#,
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
                per_visit: item.per_visit,
            }),
            PriceMode::Refresh => None,
        };

        treatment_items::insert_pinned_item(
            transaction,
            new_id,
            item.kind,
            reference_id,
            item.quantity,
            // The visit's line stays the visit's in the copy, also one from before its service
            // decided (0024): with one animal, "unsaid" would file it under that animal.
            item.patient_id
                .and_then(|patient| records.get(&patient).copied())
                .map_or(Attribution::Visit, Attribution::Record),
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
        r#"SELECT treatment.id, treatment.appointment_id, treatment.customer_id,
                  treatment.created_at, treatment.updated_at,
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
        r#"SELECT record.id,
                  record.treatment_reason AS "treatment_reason: Jsonb<RichText>",
                  record.finding AS "finding: Jsonb<RichText>",
                  patient.id AS patient_id, patient.name, patient.customer_id,
                  patient.warning_remark
           FROM patient_treatment record
           JOIN patient ON patient.id = record.patient_id
           WHERE record.treatment_id = $1
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

    // Read, no longer guessed: it used to be `patients.first()` after ordering by name, which
    // silently picked an owner when a treatment held more than one.
    let customer_id = row.customer_id;
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

    // The PDF is a file with a timestamp; the treatment carries one too, kept current by the
    // triggers on its lines and its patients. Anything newer than the file is not in it.
    let pdf_rendered_at = sqlx::query_scalar!(
        "SELECT attachment.created_at
           FROM invoice
           JOIN attachment ON attachment.id = invoice.pdf_attachment_id
          WHERE invoice.treatment_id = $1 AND invoice.status <> 'cancelled'
          ORDER BY invoice.id DESC
          LIMIT 1",
        id,
    )
    .fetch_optional(&mut *connection)
    .await?;
    let pdf_stale = pdf_rendered_at.is_some_and(|rendered| row.updated_at > rendered);

    Ok(Treatment {
        id: row.id,
        appointment_id: row.appointment_id,
        starts_at: row.starts_at,
        patients: patients
            .into_iter()
            .map(|patient| TreatmentPatient {
                id: patient.id,
                patient_id: patient.patient_id,
                name: patient.name.unwrap_or_default(),
                customer_id: patient.customer_id.unwrap_or_default(),
                warning_remark: patient.warning_remark,
                treatment_reason: patient.treatment_reason.map(|document| document.0),
                finding: patient.finding.map(|document| document.0),
            })
            .collect(),
        customer_id,
        customer_emails,
        invoice,
        item_count: i64::try_from(items.len()).unwrap_or(i64::MAX),
        frozen,
        pdf_stale,
        total_gross: money::total_gross(&groups),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Who a treatment concerns: the customer's display name and the animals', in the order lists
/// and documents show them. The customer comes from the animals — a treatment has no other.
#[derive(Debug)]
pub struct Parties {
    pub customer_id: Option<i64>,
    pub customer_name: String,
    pub patients: Vec<String>,
}

pub async fn parties(pool: &sqlx::PgPool, treatment_id: i64) -> AppResult<Parties> {
    let rows = sqlx::query!(
        r#"SELECT patient.name, patient.customer_id, customer.first_name, customer.last_name
           FROM patient_treatment
           JOIN patient ON patient.id = patient_treatment.patient_id
           LEFT JOIN customer ON customer.id = patient.customer_id
           WHERE patient_treatment.treatment_id = $1
           ORDER BY patient.name"#,
        treatment_id,
    )
    .fetch_all(pool)
    .await?;

    let customer_name = rows
        .first()
        .map(|row| {
            [row.first_name.clone(), row.last_name.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    Ok(Parties {
        customer_id: rows.first().and_then(|row| row.customer_id),
        customer_name,
        patients: rows
            .iter()
            .map(|row| row.name.clone().unwrap_or_default())
            .collect(),
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/treatments/{id}", get(detail))
        .route("/treatments/{id}/patients", post(add_patient))
        .route(
            "/treatments/{id}/patients/{patient_id}",
            axum::routing::delete(remove_patient),
        )
        .route(
            "/treatments/{id}/patients/{patient_id}/own-appointment",
            post(move_to_own_appointment),
        )
        .route(
            "/treatments/{id}/items",
            get(treatment_items::list).post(treatment_items::create),
        )
        .route(
            "/treatments/{id}/items/bulk-delete",
            post(treatment_items::bulk_delete),
        )
        .route("/treatments/{id}/apply-template", post(apply_template))
}
