//! Invoices: creation from a treatment, PDF, acceptance with email, cancellation (T035),
//! and the bookkeeping hand-off — list, submit, bulk-submit, pending-PDF bundle (T068).
//!
//! Lifecycle rules that must hold (FR-030…FR-035):
//!   * an invoice is only ever created or updated **from a treatment**
//!   * updating a `created` invoice keeps its number; updating a released one cancels it
//!     first — its number is burned and never reissued
//!   * accepting freezes the treatment's dispense movements; cancelling writes linked
//!     compensating corrections
//!   * `created → accepted → sent → submitted`: the customer gets the invoice before the
//!     bookkeeper does, whether by email or on paper
//!   * every lifecycle step is stamped with a timestamp as the audit trail

use std::io::{Cursor, Write};

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use utoipa::{IntoParams, ToSchema};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::AppState;
use crate::api::{treatment_items, treatments};
use crate::domain::enums::{InvoiceStatus, PackagingKind, Salutation};
use crate::domain::files::BytesSource;
use crate::domain::invoice_number::{NumberPattern, allocate_number, validate_pattern};
use crate::domain::{giro, money, stock};
use crate::error::{AppError, AppResult};
use crate::mail::{InvoiceMail, OutgoingInvoice, greeting};
use crate::pdf::{
    self, CustomerAddress, InvoiceBlock, InvoiceDocument, InvoiceLine, PatientGroup, PracticeBlock,
    date_de, money_de, number_de, percent_de,
};

/// Retries when a rendered number collides with a historical one (pattern flip-flop).
const NUMBER_ALLOCATION_ATTEMPTS: usize = 5;
/// A practice writes a few hundred invoices a year, so one page is the whole list.
const LIST_LIMIT: i64 = 500;

#[derive(Debug, Serialize, ToSchema)]
pub struct Invoice {
    pub id: i64,
    pub treatment_id: i64,
    pub invoice_number: String,
    pub invoice_date: NaiveDate,
    pub status: InvoiceStatus,
    pub includes_finding: bool,
    pub note: Option<String>,
    pub pdf_attachment_id: Option<i64>,
    pub email_recipients: Vec<String>,
    pub ts_accepted: Option<DateTime<Utc>>,
    pub ts_sent_email: Option<DateTime<Utc>>,
    /// When the invoice was handed over on paper — the only trace a postal send leaves.
    pub ts_sent_post: Option<DateTime<Utc>>,
    pub ts_submitted: Option<DateTime<Utc>>,
    pub ts_cancelled: Option<DateTime<Utc>>,
    pub customer_id: Option<i64>,
    /// Display name of the customer, for lists.
    pub customer_name: String,
    pub patients: Vec<String>,
    pub total_gross: Decimal,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreateInvoice {
    /// Prints the treatment's finding on the invoice.
    #[serde(default)]
    pub includes_finding: bool,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AcceptInvoice {
    /// Customer email addresses the PDF is sent to.
    #[serde(default)]
    pub recipient_emails: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SendInvoice {
    /// Where this send goes. Replaces the stored recipients, so a mistyped address can be
    /// corrected and an invoice accepted without one can still be emailed.
    #[serde(default)]
    pub recipient_emails: Vec<String>,
}

#[utoipa::path(
    post,
    operation_id = "createInvoice",
    path = "/api/treatments/{id}/invoice",
    tag = "invoices",
    params(("id" = i64, Path,)),
    request_body = CreateInvoice,
    responses(
        (status = 200, description = "Invoice in status `created`", body = Invoice),
        (status = 422, description = "The treatment cannot be billed yet")
    )
)]
pub async fn create_or_update(
    State(state): State<AppState>,
    Path(treatment_id): Path<i64>,
    Json(body): Json<CreateInvoice>,
) -> AppResult<Json<Invoice>> {
    let pattern = validate_pattern(&state.config.invoice.number_pattern)
        .map_err(|error| AppError::internal("invoice number pattern", error))?;

    let mut transaction = state.pool.begin().await?;

    let existing = sqlx::query!(
        r#"SELECT id, invoice_number, invoice_date, status AS "status: InvoiceStatus"
           FROM invoice WHERE treatment_id = $1 AND status <> 'cancelled' FOR UPDATE"#,
        treatment_id,
    )
    .fetch_optional(&mut *transaction)
    .await?;

    let invoice_id = match existing {
        // A `created` invoice keeps its number and date (FR-032).
        Some(current) if current.status == InvoiceStatus::Created => {
            sqlx::query!(
                "UPDATE invoice SET includes_finding = $2, note = $3 WHERE id = $1",
                current.id,
                body.includes_finding,
                body.note,
            )
            .execute(&mut *transaction)
            .await?;
            current.id
        }
        // An accepted invoice is cancelled first; its number is burned.
        Some(current) => {
            cancel_in_transaction(&mut transaction, current.id, treatment_id).await?;
            insert_invoice(
                &mut transaction,
                &pattern,
                treatment_id,
                &body,
                state.config.invoice.payment_terms_days,
            )
            .await?
        }
        None => {
            insert_invoice(
                &mut transaction,
                &pattern,
                treatment_id,
                &body,
                state.config.invoice.payment_terms_days,
            )
            .await?
        }
    };

    let document = build_document(&mut transaction, &state, invoice_id).await?;
    transaction.commit().await?;

    // Rendering and storing the PDF happens outside the transaction: it touches the
    // filesystem, and a failed render must not roll back a legitimate invoice number.
    attach_pdf(&state, invoice_id, document).await?;

    load(&state.pool, invoice_id).await.map(Json)
}

/// Allocates a number and inserts the invoice, retrying on a number collision.
async fn insert_invoice(
    transaction: &mut Transaction<'_, Postgres>,
    pattern: &NumberPattern,
    treatment_id: i64,
    body: &CreateInvoice,
    payment_terms_days: i64,
) -> AppResult<i64> {
    // Billing needs at least one line and a patient (the invoice's customer).
    let lines: i64 = sqlx::query_scalar!(
        "SELECT count(*) FROM treatment_item WHERE treatment_id = $1",
        treatment_id
    )
    .fetch_one(&mut **transaction)
    .await?
    .unwrap_or(0);
    if lines == 0 {
        return Err(AppError::field("items", "invoice.noLines"));
    }
    let patients: i64 = sqlx::query_scalar!(
        "SELECT count(*) FROM patient_treatment WHERE treatment_id = $1",
        treatment_id
    )
    .fetch_one(&mut **transaction)
    .await?
    .unwrap_or(0);
    if patients == 0 {
        return Err(AppError::field("patients", "invoice.noPatient"));
    }

    // The invoice date is set now, and anew on every replacement invoice (FR-030).
    let invoice_date = Local::now().date_naive();
    // Pinned rather than derived on read: changing the configured term later must not move the
    // due date of an invoice that has already gone out.
    let due_date = invoice_date
        .checked_add_signed(chrono::Duration::days(payment_terms_days))
        .unwrap_or(invoice_date);

    for attempt in 1..=NUMBER_ALLOCATION_ATTEMPTS {
        let number = allocate_number(&mut **transaction, pattern, invoice_date).await?;
        let inserted = sqlx::query_scalar!(
            "INSERT INTO invoice (treatment_id, invoice_number, invoice_date, due_date,
                                  includes_finding, note)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (invoice_number) DO NOTHING
             RETURNING id",
            treatment_id,
            number,
            invoice_date,
            due_date,
            body.includes_finding,
            body.note,
        )
        .fetch_optional(&mut **transaction)
        .await?;

        match inserted {
            Some(id) => return Ok(id),
            None => tracing::warn!(
                number,
                attempt,
                "invoice number already used — allocating the next one"
            ),
        }
    }
    Err(AppError::Internal(
        "could not allocate a free invoice number".to_owned(),
    ))
}

#[utoipa::path(
    get,
    operation_id = "getInvoice",
    path = "/api/invoices/{id}",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Invoice), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Invoice>> {
    load(&state.pool, id).await.map(Json)
}

#[utoipa::path(
    get,
    operation_id = "invoicePdf",
    path = "/api/invoices/{id}/pdf",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses((status = 200, description = "The invoice PDF"), (status = 404))
)]
pub async fn pdf(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let row = sqlx::query!(
        r#"SELECT invoice.invoice_number, attachment.sha256 AS "sha256?"
           FROM invoice
           LEFT JOIN attachment ON attachment.id = invoice.pdf_attachment_id
           WHERE invoice.id = $1"#,
        id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let sha256 = row.sha256.ok_or(AppError::NotFound)?;
    let bytes = tokio::fs::read(state.files.content_path(&sha256)).await?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/pdf".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"Rechnung-{}.pdf\"", row.invoice_number),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[utoipa::path(
    post,
    operation_id = "acceptInvoice",
    path = "/api/invoices/{id}/accept",
    tag = "invoices",
    params(("id" = i64, Path,)),
    request_body = AcceptInvoice,
    responses(
        (status = 200, description = "Accepted; the email is sent if SMTP accepts it", body = Invoice),
        (status = 409, description = "Only a created invoice can be accepted")
    )
)]
pub async fn accept(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AcceptInvoice>,
) -> AppResult<Json<Invoice>> {
    let current = sqlx::query!(
        r#"SELECT status AS "status: InvoiceStatus", treatment_id FROM invoice WHERE id = $1"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if current.status != InvoiceStatus::Created {
        return Err(AppError::Conflict(
            "only a created invoice can be accepted".to_owned(),
        ));
    }

    // Acceptance is the freeze point: from here the stock ledger is append-only.
    sqlx::query!(
        "UPDATE invoice
         SET status = 'accepted', ts_accepted = now(), email_recipients = $2
         WHERE id = $1",
        id,
        &body.recipient_emails,
    )
    .execute(&state.pool)
    .await?;

    // A failing send leaves the invoice accepted and retriable (FR-031).
    if !body.recipient_emails.is_empty()
        && let Err(error) = send_invoice_email(&state, id).await
    {
        tracing::error!(%error, invoice_id = id, "sending the invoice email failed");
    }

    load(&state.pool, id).await.map(Json)
}

#[utoipa::path(
    post,
    operation_id = "sendInvoice",
    path = "/api/invoices/{id}/send",
    tag = "invoices",
    params(("id" = i64, Path,)),
    request_body = SendInvoice,
    responses(
        (status = 200, description = "Sent; the invoice is now `sent`", body = Invoice),
        (status = 409, description = "The invoice is not released, or is cancelled"),
        (status = 422, description = "No recipient was given"),
        (status = 500, description = "The mail server rejected the message")
    )
)]
pub async fn send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<SendInvoice>,
) -> AppResult<Json<Invoice>> {
    let current = sqlx::query!(
        r#"SELECT status AS "status: InvoiceStatus" FROM invoice WHERE id = $1"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // A sent invoice may be sent again — that is when a resend is usually wanted.
    if matches!(
        current.status,
        InvoiceStatus::Created | InvoiceStatus::Cancelled
    ) {
        return Err(AppError::Conflict(
            "only a released invoice can be emailed".to_owned(),
        ));
    }

    let recipients: Vec<String> = body
        .recipient_emails
        .into_iter()
        .map(|email| email.trim().to_owned())
        .filter(|email| !email.is_empty())
        .collect();
    if recipients.is_empty() {
        return Err(AppError::field("recipient_emails", "field.required"));
    }

    sqlx::query!(
        "UPDATE invoice SET email_recipients = $2 WHERE id = $1",
        id,
        &recipients,
    )
    .execute(&state.pool)
    .await?;

    // Unlike accept, an explicit send reports the failure to the caller.
    send_invoice_email(&state, id).await?;
    load(&state.pool, id).await.map(Json)
}

#[utoipa::path(
    post,
    operation_id = "markInvoicePosted",
    path = "/api/invoices/{id}/mark-posted",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses(
        (status = 200, description = "Recorded as handed over on paper", body = Invoice),
        (status = 409, description = "Only a released, unsent invoice can be marked as posted")
    )
)]
pub async fn mark_posted(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Invoice>> {
    // Nothing else can observe a letter going into a postbox, so the vet says so here.
    let updated = sqlx::query_scalar!(
        "UPDATE invoice SET status = 'sent', ts_sent_post = now()
         WHERE id = $1 AND status = 'accepted'
         RETURNING id",
        id,
    )
    .fetch_optional(&state.pool)
    .await?;

    if updated.is_none() {
        // Distinguish "no such invoice" from "wrong stage" — the UI shows different things.
        load(&state.pool, id).await?;
        return Err(AppError::Conflict(
            "only a released invoice that has not been sent can be marked as posted".to_owned(),
        ));
    }

    load(&state.pool, id).await.map(Json)
}

#[utoipa::path(
    post,
    operation_id = "cancelInvoice",
    path = "/api/invoices/{id}/cancel",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses(
        (status = 200, body = Invoice),
        (status = 409, description = "The invoice is already cancelled")
    )
)]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Invoice>> {
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query!(
        r#"SELECT status AS "status: InvoiceStatus", treatment_id FROM invoice WHERE id = $1
           FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if current.status == InvoiceStatus::Cancelled {
        return Err(AppError::Conflict(
            "the invoice is already cancelled".to_owned(),
        ));
    }

    cancel_in_transaction(&mut transaction, id, current.treatment_id).await?;
    transaction.commit().await?;

    load(&state.pool, id).await.map(Json)
}

/// Cancels an invoice and compensates its frozen dispenses.
async fn cancel_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    invoice_id: i64,
    treatment_id: i64,
) -> AppResult<()> {
    let status = sqlx::query_scalar!(
        r#"SELECT status AS "status: InvoiceStatus" FROM invoice WHERE id = $1"#,
        invoice_id
    )
    .fetch_one(&mut **transaction)
    .await?;

    sqlx::query!(
        "UPDATE invoice SET status = 'cancelled', ts_cancelled = now() WHERE id = $1",
        invoice_id
    )
    .execute(&mut **transaction)
    .await?;

    // Only frozen (released) dispenses need reversing — draft ones were never booked.
    if matches!(
        status,
        InvoiceStatus::Accepted | InvoiceStatus::Sent | InvoiceStatus::Submitted
    ) {
        let reversed = stock::reverse_treatment_dispenses(transaction, treatment_id).await?;
        tracing::info!(
            invoice_id,
            reversed,
            "invoice cancelled, dispenses compensated"
        );
    }
    Ok(())
}

/// Renders the invoice email and sends it; records the timestamp only on success.
async fn send_invoice_email(state: &AppState, invoice_id: i64) -> AppResult<()> {
    let row = sqlx::query!(
        r#"SELECT invoice.invoice_number, invoice.invoice_date, invoice.email_recipients,
                  attachment.sha256 AS "sha256?",
                  customer.salutation AS "salutation?: Salutation",
                  customer.first_name, customer.last_name,
                  customer.second_salutation AS "second_salutation?: Salutation",
                  customer.second_first_name, customer.second_last_name,
                  customer.invoice_salutation AS "invoice_salutation?: Salutation",
                  customer.invoice_first_name, customer.invoice_last_name,
                  customer.has_invoice_address,
                  settings.practice_name, settings.cc_emails, settings.bcc_emails
           FROM invoice
           LEFT JOIN attachment ON attachment.id = invoice.pdf_attachment_id
           JOIN treatment ON treatment.id = invoice.treatment_id
           LEFT JOIN patient_treatment ON patient_treatment.treatment_id = treatment.id
           LEFT JOIN patient ON patient.id = patient_treatment.patient_id
           LEFT JOIN customer ON customer.id = patient.customer_id
           CROSS JOIN global_settings settings
           WHERE invoice.id = $1
           LIMIT 1"#,
        invoice_id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let recipients = row.email_recipients.unwrap_or_default();
    if recipients.is_empty() {
        return Err(AppError::field("recipient_emails", "field.required"));
    }

    let patients = sqlx::query_scalar!(
        "SELECT patient.name
         FROM invoice
         JOIN patient_treatment ON patient_treatment.treatment_id = invoice.treatment_id
         JOIN patient ON patient.id = patient_treatment.patient_id
         WHERE invoice.id = $1
         ORDER BY patient.name",
        invoice_id,
    )
    .fetch_all(&state.pool)
    .await?;

    let total = invoice_total(&state.pool, invoice_id).await?;

    // The letter must greet whoever the PDF is addressed to. With a separate invoice address
    // that is the invoice recipient, and the household's second name does not belong to them.
    let addressed_to_invoice_recipient = row.has_invoice_address;
    let (salutation, first_name, last_name) = if addressed_to_invoice_recipient {
        (
            row.invoice_salutation,
            row.invoice_first_name.unwrap_or_default(),
            row.invoice_last_name.unwrap_or_default(),
        )
    } else {
        (
            row.salutation,
            row.first_name.unwrap_or_default(),
            row.last_name.unwrap_or_default(),
        )
    };
    let (second_salutation, second_first_name, second_last_name) = if addressed_to_invoice_recipient
    {
        (None, String::new(), String::new())
    } else {
        (
            row.second_salutation,
            row.second_first_name.unwrap_or_default(),
            row.second_last_name.unwrap_or_default(),
        )
    };

    let context = InvoiceMail {
        greeting: greeting(salutation, &last_name, second_salutation, &second_last_name),
        salutation: salutation
            .map(|value| value.to_string())
            .unwrap_or_default(),
        first_name,
        last_name,
        second_salutation: second_salutation
            .map(|value| value.to_string())
            .unwrap_or_default(),
        second_first_name,
        second_last_name,
        invoice_number: row.invoice_number.clone(),
        invoice_date: date_de(row.invoice_date),
        invoice_total: money_de(total, &state.config.invoice.currency),
        practice_name: row.practice_name,
        patients: patients.into_iter().flatten().collect(),
    };

    let (subject, body) = state.mailer.render(&context)?;
    let sha256 = row
        .sha256
        .ok_or_else(|| AppError::Internal("the invoice has no rendered PDF to send".to_owned()))?;
    let pdf_bytes = tokio::fs::read(state.files.content_path(&sha256)).await?;

    let message = state.mailer.build_message(OutgoingInvoice {
        recipients: &recipients,
        cc: &row.cc_emails,
        bcc: &row.bcc_emails,
        subject: &subject,
        body: &body,
        pdf_name: &format!("Rechnung-{}.pdf", row.invoice_number),
        pdf: pdf_bytes,
    })?;
    state.mailer.send(message).await?;

    // The timestamp is written only after the mail server accepted the message, and it is what
    // moves the invoice on. A resend must not drag a submitted invoice back a stage.
    sqlx::query!(
        "UPDATE invoice
         SET ts_sent_email = now(),
             status = CASE WHEN status = 'accepted' THEN 'sent'::invoice_status ELSE status END
         WHERE id = $1",
        invoice_id
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Renders the PDF and links it to the invoice as a content-addressed attachment.
async fn attach_pdf(state: &AppState, invoice_id: i64, document: InvoiceDocument) -> AppResult<()> {
    let logo = load_logo(state).await?;
    let config = std::sync::Arc::clone(&state.config);
    let qr = document.invoice.qr_payload.as_deref().and_then(giro_svg);

    // Typesetting is CPU-bound — keep it off the async worker threads.
    let bytes =
        tokio::task::spawn_blocking(move || pdf::render_invoice(&config, &document, logo, qr))
            .await
            .map_err(|error| AppError::internal("joining the PDF task", error))??;

    let number = sqlx::query_scalar!(
        "SELECT invoice_number FROM invoice WHERE id = $1",
        invoice_id
    )
    .fetch_one(&state.pool)
    .await?;

    let stored = state
        .files
        .store(BytesSource::new(bytes.clone()), u64::MAX)
        .await?;
    let attachment_id: i64 = sqlx::query_scalar!(
        r#"INSERT INTO attachment
               (sha256, mime_type, size_bytes, orig_name, kind)
           VALUES ($1, 'application/pdf', $2, $3, 'referenced')
           RETURNING id"#,
        stored.sha256,
        stored.size_bytes,
        format!("Rechnung-{number}.pdf"),
    )
    .fetch_one(&state.pool)
    .await?;

    sqlx::query!(
        "UPDATE invoice SET pdf_attachment_id = $2 WHERE id = $1",
        invoice_id,
        attachment_id
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn load_logo(state: &AppState) -> AppResult<Option<Vec<u8>>> {
    let sha256 = sqlx::query_scalar!(
        "SELECT attachment.sha256
         FROM global_settings
         JOIN attachment ON attachment.id = global_settings.logo_attachment_id",
    )
    .fetch_optional(&state.pool)
    .await?;

    match sha256 {
        Some(sha256) => match tokio::fs::read(state.files.content_path(&sha256)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) => {
                tracing::warn!(%error, "practice logo is missing on disk, rendering without it");
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Assembles everything the invoice template prints, formatted German.
async fn build_document(
    connection: &mut PgConnection,
    state: &AppState,
    invoice_id: i64,
) -> AppResult<InvoiceDocument> {
    let currency = state.config.invoice.currency.clone();

    let invoice = sqlx::query!(
        r#"SELECT invoice.invoice_number, invoice.invoice_date, invoice.due_date,
                  invoice.includes_finding,
                  invoice.note, invoice.treatment_id,
                  appointment.starts_at
           FROM invoice
           JOIN treatment ON treatment.id = invoice.treatment_id
           JOIN appointment ON appointment.id = treatment.appointment_id
           WHERE invoice.id = $1"#,
        invoice_id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let settings = sqlx::query!(
        "SELECT practice_name, practice_street, practice_zip, practice_city, email, iban,
                bic, bank_name, ustid, logo_attachment_id
         FROM global_settings LIMIT 1",
    )
    .fetch_one(&mut *connection)
    .await?;

    let patients = sqlx::query!(
        r#"SELECT patient_treatment.id, patient_treatment.treatment_reason,
                  patient_treatment.finding,
                  patient.name, patient.customer_id, patient.species, patient.race,
                  patient.date_of_birth
           FROM patient_treatment
           JOIN patient ON patient.id = patient_treatment.patient_id
           WHERE patient_treatment.treatment_id = $1
           ORDER BY patient.name"#,
        invoice.treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    let customer_id = patients
        .first()
        .and_then(|patient| patient.customer_id)
        .ok_or_else(|| AppError::field("patients", "invoice.noPatient"))?;
    let recipient = recipient_lines(&mut *connection, customer_id).await?;
    let greeting = recipient.greeting.clone();

    let items = treatment_items::load_items(&mut *connection, invoice.treatment_id).await?;
    let patient_names: std::collections::HashMap<i64, String> = patients
        .iter()
        .map(|patient| (patient.id, patient.name.clone().unwrap_or_default()))
        .collect();

    // Packaging kind and marketing authorisation number for the grey sub-label; neither is
    // pinned on the line, so they are read from the catalog at render time.
    let packaging_ids: Vec<i64> = items
        .iter()
        .filter_map(|item| item.drug_packaging_id)
        .collect();
    let packagings: Vec<PackagingDetail> = sqlx::query!(
        r#"SELECT packaging.id, packaging.kind AS "kind: PackagingKind", drug.approval_number
           FROM drug_packaging packaging
           JOIN drug ON drug.id = packaging.drug_id
           WHERE packaging.id = ANY($1)"#,
        &packaging_ids,
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(|row| PackagingDetail {
        id: row.id,
        kind: row.kind,
        approval_number: row.approval_number,
    })
    .collect();

    let groups = money::vat_summary(
        &items
            .iter()
            .map(|item| (item.line_net, item.vat_percent))
            .collect::<Vec<_>>(),
    );

    let lines: Vec<InvoiceLine> = items
        .iter()
        .map(|item| InvoiceLine {
            position: item.position,
            name: item.name.clone(),
            // Only worth printing when the invoice covers more than one animal.
            patient: if patients.len() > 1 {
                item.patient_treatment_id
                    .and_then(|id| patient_names.get(&id).cloned())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            quantity: number_de(item.quantity),
            unit: item.unit.clone().unwrap_or_default(),
            factor: item.factor.map(percent_de).unwrap_or_default(),
            got_number: item.got_number.clone().unwrap_or_default(),
            km: item.km.map(number_de).unwrap_or_default(),
            vat: percent_de(item.vat_percent),
            detail: line_detail(item, &packagings),
            price: money_de(item.price_gross, &currency),
            // Exact by construction: VAT is rounded per line, so `net + vat` needs no further
            // rounding and the column sums to the VAT group's gross (`money::vat_summary`).
            total: money_de(item.line_gross, &currency),
        })
        .collect();

    let treatment_date = invoice
        .starts_at
        .map(|starts_at| date_de(starts_at.with_timezone(&Local).date_naive()))
        .unwrap_or_default();

    let address_lines: Vec<String> = [
        settings.practice_street.trim(),
        &format!(
            "{} {}",
            settings.practice_zip.trim(),
            settings.practice_city.trim()
        ),
    ]
    .into_iter()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(str::to_owned)
    .collect();

    let descriptions: std::collections::HashMap<i64, String> = patients
        .iter()
        .map(|patient| {
            (
                patient.id,
                patient_description(
                    patient.species.as_deref(),
                    patient.race.as_deref(),
                    patient.date_of_birth,
                ),
            )
        })
        .collect();

    let patient_pairs: Vec<(i64, String)> = patients
        .iter()
        .map(|patient| (patient.id, patient.name.clone().unwrap_or_default()))
        .collect();

    let total = money::total_gross(&groups);
    // No GiroCode is not an error: an empty IBAN or a non-euro currency is ordinary
    // configuration, and it should cost the invoice its QR code, not its render.
    let qr_payload = giro::epc_payload(
        &settings.practice_name,
        &settings.iban,
        &settings.bic,
        total,
        &format!("Rechnung {}", invoice.invoice_number),
        &currency,
    );

    Ok(InvoiceDocument {
        practice: PracticeBlock {
            name: settings.practice_name.clone(),
            address: address_lines.join("\n"),
            address_line: address_lines.join(", "),
            email: settings.email,
            iban: settings.iban,
            bic: settings.bic,
            bank_name: settings.bank_name,
            ustid: settings.ustid,
            logo_present: settings.logo_attachment_id.is_some(),
        },
        invoice: InvoiceBlock {
            number: invoice.invoice_number,
            date: date_de(invoice.invoice_date),
            due_date: invoice.due_date.map(date_de).unwrap_or_default(),
            treatment_date: treatment_date.clone(),
            recipient: recipient.lines,
            sender_line: [settings.practice_name, address_lines.join(", ")]
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", "),
            greeting,
            patients: patients
                .iter()
                .map(|patient| patient.name.clone().unwrap_or_default())
                .collect(),
            treatment_heading: treatment_heading(&patient_pairs, &descriptions, &treatment_date),
            // One report per animal, in the order the groups print. An animal with neither a
            // reason nor a finding is left out rather than printed as a bare name, and the
            // finding is only there when the vet asked for it (FR-030).
            reports: patients
                .iter()
                .filter_map(|patient| {
                    let reason = patient.treatment_reason.clone().unwrap_or_default();
                    let finding = if invoice.includes_finding {
                        patient.finding.clone().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    if reason.trim().is_empty() && finding.trim().is_empty() {
                        return None;
                    }
                    Some(pdf::PatientReport {
                        patient: patient.name.clone().unwrap_or_default(),
                        description: descriptions.get(&patient.id).cloned().unwrap_or_default(),
                        treatment_reason: reason,
                        finding,
                    })
                })
                .collect(),
            patient_groups: group_by_patient(
                &lines,
                &items,
                &patient_names,
                &descriptions,
                &treatment_date,
            ),
            items: lines,
            vat_groups: pdf::vat_groups_de(&groups, &currency),
            total: money_de(total, &currency),
            note: invoice.note.unwrap_or_default(),
            qr_present: qr_payload.is_some(),
            qr_payload,
        },
    })
}

/// Renders the GiroCode payload to an SVG for the template.
fn giro_svg(payload: &str) -> Option<String> {
    use fast_qr::convert::{Builder, Shape, svg::SvgBuilder};

    // Error-correction level M is what EPC069-12 recommends for a GiroCode.
    let code = fast_qr::QRBuilder::new(payload)
        .ecl(fast_qr::ECL::M)
        .build()
        .inspect_err(|error| tracing::warn!(%error, "the GiroCode payload did not fit a QR code"))
        .ok()?;
    Some(SvgBuilder::default().shape(Shape::Square).to_str(&code))
}

/// Catalog facts a drug line prints but does not pin: what kind of packaging it was and, for an
/// original, the marketing authorisation number.
struct PackagingDetail {
    id: i64,
    kind: PackagingKind,
    approval_number: Option<String>,
}

/// `Hund, Havaneser, Geburtsdatum: 01.01.2021` — empty parts are simply left out.
fn patient_description(
    species: Option<&str>,
    race: Option<&str>,
    date_of_birth: Option<chrono::NaiveDate>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(
        species
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    );
    parts.extend(
        race.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    );
    parts.extend(date_of_birth.map(|date| format!("Geburtsdatum: {}", date_de(date))));
    parts.join(", ")
}

/// The grey second line under a billing position.
fn line_detail(item: &treatment_items::TreatmentItem, packagings: &[PackagingDetail]) -> String {
    if let Some(number) = item.got_number.as_deref().map(str::trim)
        && !number.is_empty()
    {
        return format!("GOT-Nr: {number}");
    }
    let Some(packaging_id) = item.drug_packaging_id else {
        return String::new();
    };
    let Some(packaging) = packagings.iter().find(|row| row.id == packaging_id) else {
        return String::new();
    };
    match packaging.kind {
        PackagingKind::Original => {
            let label = format!("{} Originalpackung", number_de(item.quantity));
            match packaging.approval_number.as_deref().map(str::trim) {
                Some(approval) if !approval.is_empty() => {
                    format!("{label}, Zulassungsnr: {approval}")
                }
                _ => label,
            }
        }
        PackagingKind::Subset => match item.unit.as_deref().map(str::trim) {
            Some(unit) if !unit.is_empty() => format!("{} {unit}", number_de(item.quantity)),
            _ => number_de(item.quantity),
        },
    }
}

/// `Behandlung/Konsultation Eddie (Hund – Havaneser) am 28.07.2026`.
fn treatment_heading(
    patients: &[(i64, String)],
    descriptions: &std::collections::HashMap<i64, String>,
    treatment_date: &str,
) -> String {
    if patients.is_empty() {
        return String::new();
    }
    let named: Vec<String> = patients
        .iter()
        .map(|(id, name)| match descriptions.get(id) {
            // The heading reads "Eddie (Hund – Havaneser)"; the date of birth is already in the
            // table above, so it is dropped here.
            Some(description) if !description.is_empty() => format!(
                "{name} ({})",
                description
                    .split(", Geburtsdatum:")
                    .next()
                    .unwrap_or(description)
                    .replace(", ", " – "),
            ),
            _ => name.clone(),
        })
        .collect();
    let who = named.join(", ");
    if treatment_date.is_empty() {
        format!("Behandlung/Konsultation {who}")
    } else {
        format!("Behandlung/Konsultation {who} am {treatment_date}")
    }
}

/// Groups the printable lines per animal, keeping the vet's ordering.
///
/// Groups appear in the order their first line does, and lines keep their order inside a group,
/// so reordering positions in the treatment reorders the invoice the same way. Lines with no
/// animal (a service the vet left unattributed) collect into a trailing group with an empty
/// name, which the template prints without a `Tier:` heading.
fn group_by_patient(
    lines: &[InvoiceLine],
    items: &[treatment_items::TreatmentItem],
    names: &std::collections::HashMap<i64, String>,
    descriptions: &std::collections::HashMap<i64, String>,
    service_date: &str,
) -> Vec<PatientGroup> {
    let mut groups: Vec<(Option<i64>, PatientGroup)> = Vec::new();
    for (line, item) in lines.iter().zip(items.iter()) {
        let key = item.patient_treatment_id;
        if let Some((_, group)) = groups.iter_mut().find(|(existing, _)| *existing == key) {
            group.items.push(line.clone());
            continue;
        }
        groups.push((
            key,
            PatientGroup {
                patient: key
                    .and_then(|id| names.get(&id).cloned())
                    .unwrap_or_default(),
                description: key
                    .and_then(|id| descriptions.get(&id).cloned())
                    .unwrap_or_default(),
                service_date: service_date.to_owned(),
                items: vec![line.clone()],
            },
        ));
    }
    // Unattributed lines belong at the end, after every animal.
    groups.sort_by_key(|(key, _)| key.is_none());
    groups.into_iter().map(|(_, group)| group).collect()
}

/// The recipient of this invoice: the address block and the salutation that goes with it.
struct Recipient {
    lines: Vec<String>,
    greeting: String,
}

/// Loads the customer's address columns, formats the invoice address block and the salutation.
///
/// Both follow the *invoice* recipient when the customer has a separate invoice address —
/// addressing the envelope to one person and the letter inside to another would be a bug.
async fn recipient_lines(connection: &mut PgConnection, customer_id: i64) -> AppResult<Recipient> {
    let customer = sqlx::query!(
        r#"SELECT company, invoice_company,
                  salutation AS "salutation?: Salutation", first_name, last_name,
                  second_salutation AS "second_salutation?: Salutation",
                  second_first_name, second_last_name, has_second_name,
                  home_addon, home_street, home_zip, home_city,
                  invoice_salutation AS "invoice_salutation?: Salutation",
                  invoice_first_name, invoice_last_name, invoice_addon, invoice_street,
                  invoice_zip, invoice_city, has_invoice_address
           FROM customer WHERE id = $1"#,
        customer_id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let greeting = if customer.has_invoice_address {
        // The second name belongs to the household, not to the invoice recipient.
        greeting(
            customer.invoice_salutation,
            customer.invoice_last_name.as_deref().unwrap_or_default(),
            None,
            "",
        )
    } else {
        greeting(
            customer.salutation,
            customer.last_name.as_deref().unwrap_or_default(),
            customer.second_salutation,
            customer.second_last_name.as_deref().unwrap_or_default(),
        )
    };

    let lines = pdf::address_block(&CustomerAddress {
        company: customer.company,
        invoice_company: customer.invoice_company,
        salutation: customer.salutation,
        first_name: customer.first_name,
        last_name: customer.last_name,
        second_salutation: customer.second_salutation,
        second_first_name: customer.second_first_name,
        second_last_name: customer.second_last_name,
        has_second_name: customer.has_second_name,
        home_addon: customer.home_addon,
        home_street: customer.home_street,
        home_zip: customer.home_zip,
        home_city: customer.home_city,
        invoice_salutation: customer.invoice_salutation,
        invoice_first_name: customer.invoice_first_name,
        invoice_last_name: customer.invoice_last_name,
        invoice_addon: customer.invoice_addon,
        invoice_street: customer.invoice_street,
        invoice_zip: customer.invoice_zip,
        invoice_city: customer.invoice_city,
        has_invoice_address: customer.has_invoice_address,
    });

    Ok(Recipient { lines, greeting })
}

/// Invoice total from the pinned line values.
pub async fn invoice_total(pool: &PgPool, invoice_id: i64) -> AppResult<Decimal> {
    let treatment_id: i64 =
        sqlx::query_scalar!("SELECT treatment_id FROM invoice WHERE id = $1", invoice_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let mut connection = pool.acquire().await?;
    treatment_items::treatment_total_gross(&mut connection, treatment_id).await
}

/// Loads one invoice with the display data lists and detail views need.
pub async fn load(pool: &PgPool, id: i64) -> AppResult<Invoice> {
    let row = sqlx::query!(
        r#"SELECT invoice.id, invoice.treatment_id, invoice.invoice_number, invoice.invoice_date,
                  invoice.status AS "status: InvoiceStatus", invoice.includes_finding,
                  invoice.note, invoice.pdf_attachment_id, invoice.email_recipients,
                  invoice.ts_accepted, invoice.ts_sent_email, invoice.ts_sent_post,
                  invoice.ts_submitted,
                  invoice.ts_cancelled, invoice.created_at
           FROM invoice WHERE invoice.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let parties = treatments::parties(pool, row.treatment_id).await?;

    Ok(Invoice {
        id: row.id,
        treatment_id: row.treatment_id,
        invoice_number: row.invoice_number,
        invoice_date: row.invoice_date,
        status: row.status,
        includes_finding: row.includes_finding,
        note: row.note,
        pdf_attachment_id: row.pdf_attachment_id,
        email_recipients: row.email_recipients.unwrap_or_default(),
        ts_accepted: row.ts_accepted,
        ts_sent_email: row.ts_sent_email,
        ts_sent_post: row.ts_sent_post,
        ts_submitted: row.ts_submitted,
        ts_cancelled: row.ts_cancelled,
        customer_id: parties.customer_id,
        customer_name: parties.customer_name,
        patients: parties.patients,
        total_gross: invoice_total(pool, id).await?,
        created_at: row.created_at,
    })
}

/// Filters of the invoice list. Cancelled invoices stay out unless asked for (FR-034).
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InvoiceQuery {
    /// Matches the invoice number or the customer's name.
    pub q: Option<String>,
    /// Only invoices waiting for the bookkeeper (sent, not yet submitted).
    pub pending: Option<bool>,
    /// Include cancelled invoices.
    pub cancelled: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkSubmitResult {
    /// How many invoices were handed over.
    pub submitted: i64,
    /// Their numbers, so the vet can tick them off against the bookkeeper's list.
    pub invoice_numbers: Vec<String>,
}

#[utoipa::path(
    get,
    operation_id = "listInvoices",
    path = "/api/invoices",
    tag = "invoices",
    params(InvoiceQuery),
    responses((status = 200, body = Vec<Invoice>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<InvoiceQuery>,
) -> AppResult<Json<Vec<Invoice>>> {
    let search = query
        .q
        .as_ref()
        .map(|term| term.trim().to_owned())
        .filter(|term| !term.is_empty());
    let pattern = search.as_ref().map(|term| format!("%{term}%"));

    // Waiting-for-the-bookkeeper first; within a group the newest invoice date wins.
    let ids = sqlx::query_scalar!(
        r#"SELECT invoice.id
           FROM invoice
           WHERE (invoice.status <> 'cancelled' OR $1)
             AND (NOT $2 OR invoice.status = 'sent')
             AND ($3::text IS NULL
                  OR invoice.invoice_number ILIKE $3
                  OR EXISTS (
                       SELECT 1 FROM patient_treatment
                       JOIN patient ON patient.id = patient_treatment.patient_id
                       JOIN customer ON customer.id = patient.customer_id
                       WHERE patient_treatment.treatment_id = invoice.treatment_id
                         AND concat_ws(' ', customer.first_name, customer.last_name) ILIKE $3))
           ORDER BY (invoice.status = 'sent') DESC, invoice.invoice_date DESC, invoice.id DESC
           LIMIT $4 OFFSET $5"#,
        query.cancelled.unwrap_or(false),
        query.pending.unwrap_or(false),
        pattern.as_deref(),
        query.limit.unwrap_or(LIST_LIMIT).clamp(1, LIST_LIMIT),
        query.offset.unwrap_or(0).max(0),
    )
    .fetch_all(&state.pool)
    .await?;

    // Loading row by row keeps the listed total identical to the detail view and the PDF,
    // which compute it from the pinned lines per VAT group. Lists here are a page long.
    let mut invoices = Vec::with_capacity(ids.len());
    for id in ids {
        invoices.push(load(&state.pool, id).await?);
    }
    Ok(Json(invoices))
}

#[utoipa::path(
    post,
    operation_id = "submitInvoice",
    path = "/api/invoices/{id}/submit",
    tag = "invoices",
    params(("id" = i64, Path,)),
    responses(
        (status = 200, body = Invoice),
        (status = 409, description = "Only a sent invoice can be submitted")
    )
)]
pub async fn submit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Invoice>> {
    // The bookkeeper only ever receives invoices the customer already has.
    let updated = sqlx::query_scalar!(
        "UPDATE invoice SET status = 'submitted', ts_submitted = now()
         WHERE id = $1 AND status = 'sent'
         RETURNING id",
        id,
    )
    .fetch_optional(&state.pool)
    .await?;

    if updated.is_none() {
        // Distinguish "no such invoice" from "wrong stage" — the UI shows different things.
        load(&state.pool, id).await?;
        return Err(AppError::Conflict(
            "only a sent invoice can be submitted to bookkeeping".to_owned(),
        ));
    }

    load(&state.pool, id).await.map(Json)
}

#[utoipa::path(
    post,
    operation_id = "bulkSubmitInvoices",
    path = "/api/invoices/bulk-submit",
    tag = "invoices",
    responses((status = 200, body = BulkSubmitResult))
)]
pub async fn bulk_submit(State(state): State<AppState>) -> AppResult<Json<BulkSubmitResult>> {
    // One statement, one timestamp: the whole month is handed over as a single act.
    let invoice_numbers = sqlx::query_scalar!(
        "UPDATE invoice SET status = 'submitted', ts_submitted = now()
         WHERE status = 'sent'
         RETURNING invoice_number",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut invoice_numbers = invoice_numbers;
    invoice_numbers.sort();
    Ok(Json(BulkSubmitResult {
        submitted: i64::try_from(invoice_numbers.len()).unwrap_or(i64::MAX),
        invoice_numbers,
    }))
}

#[utoipa::path(
    get,
    operation_id = "pendingInvoicePdfs",
    path = "/api/invoices/pending-pdfs",
    tag = "invoices",
    responses(
        (status = 200, description = "ZIP of every pending invoice PDF"),
        (status = 409, description = "No invoice is waiting for the bookkeeper")
    )
)]
pub async fn pending_pdfs(State(state): State<AppState>) -> AppResult<Response> {
    let rows = sqlx::query!(
        r#"SELECT invoice.invoice_number, attachment.sha256 AS "sha256?"
           FROM invoice
           LEFT JOIN attachment ON attachment.id = invoice.pdf_attachment_id
           WHERE invoice.status = 'sent'
           ORDER BY invoice.invoice_number"#,
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Err(AppError::Conflict(
            "no invoice is waiting for the bookkeeper".to_owned(),
        ));
    }

    let mut files = Vec::with_capacity(rows.len());
    for row in rows {
        // A released invoice always has its PDF; a missing one must not fail the bundle.
        let Some(sha256) = row.sha256 else {
            tracing::error!(invoice_number = %row.invoice_number, "pending invoice without a PDF");
            continue;
        };
        let bytes = tokio::fs::read(state.files.content_path(&sha256)).await?;
        files.push((format!("Rechnung-{}.pdf", row.invoice_number), bytes));
    }

    let archive = tokio::task::spawn_blocking(move || zip_files(files))
        .await
        .map_err(|error| AppError::Internal(format!("bundling the PDFs failed: {error}")))??;

    let filename = format!("Rechnungen-{}.zip", Local::now().date_naive());
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        archive,
    )
        .into_response())
}

/// Packs named files into an in-memory ZIP. Invoice PDFs are a few dozen kilobytes each.
fn zip_files(files: Vec<(String, Vec<u8>)>) -> AppResult<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in files {
        writer
            .start_file(name, options)
            .and_then(|()| writer.write_all(&bytes).map_err(Into::into))
            .map_err(|error| AppError::Internal(format!("writing the ZIP failed: {error}")))?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|error| AppError::Internal(format!("closing the ZIP failed: {error}")))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/treatments/{id}/invoice", post(create_or_update))
        .route("/invoices", get(list))
        .route("/invoices/bulk-submit", post(bulk_submit))
        .route("/invoices/pending-pdfs", get(pending_pdfs))
        .route("/invoices/{id}", get(detail))
        .route("/invoices/{id}/pdf", get(pdf))
        .route("/invoices/{id}/accept", post(accept))
        .route("/invoices/{id}/send", post(send))
        .route("/invoices/{id}/mark-posted", post(mark_posted))
        .route("/invoices/{id}/cancel", post(cancel))
        .route("/invoices/{id}/submit", post(submit))
}
