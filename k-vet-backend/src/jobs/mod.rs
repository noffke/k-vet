//! Scheduled maintenance (T076).
//!
//! Three chores the practice should never have to think about:
//!   * the picker's usage weights, rebuilt nightly from what was actually billed
//!   * drafts that were opened by accident and never filled in, dropped after a day
//!   * uploaded files nothing points at any more, swept once a month
//!
//! Each job is a plain async function so the tests can run it directly; `spawn` is the
//! only part that deals with the clock.

use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, TimeDelta};
use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::AppState;
use crate::domain::files::AttachmentStore;
use crate::error::AppResult;

/// Nightly run time — after surgery hours, before the morning backup.
const NIGHTLY_HOUR: u32 = 3;
/// A draft the vet never typed into is abandoned after this long.
const DRAFT_MAX_AGE_HOURS: i64 = 24;
/// An upload nothing references is only swept once it is clearly not in flight.
const ORPHAN_MAX_AGE_DAYS: i64 = 30;

/// What the draft cleanup removed, per table — logged so a surprise is traceable.
#[derive(Debug, Default, Clone, Copy)]
pub struct DraftCleanup {
    pub customers: u64,
    pub patients: u64,
    pub suppliers: u64,
    pub manufacturers: u64,
    pub drugs: u64,
    pub packagings: u64,
    pub services: u64,
    pub templates: u64,
    pub appointments: u64,
}

impl DraftCleanup {
    pub fn total(&self) -> u64 {
        self.customers
            + self.patients
            + self.suppliers
            + self.manufacturers
            + self.drugs
            + self.packagings
            + self.services
            + self.templates
            + self.appointments
    }
}

/// Rebuilds the picker's usage weights from the billed lines. Returns the row count.
///
/// A full rebuild rather than incremental counting: it is a small table, and it self-heals
/// after imports, deletions and restored backups.
pub async fn refresh_picker_usage(pool: &PgPool) -> AppResult<u64> {
    let mut transaction = pool.begin().await?;

    sqlx::query!("DELETE FROM picker_usage")
        .execute(&mut *transaction)
        .await?;

    let inserted = sqlx::query!(
        "INSERT INTO picker_usage (kind, item_id, uses)
         SELECT 'drug_packaging'::treatment_item_kind, drug_packaging_id, count(*)
         FROM treatment_item WHERE drug_packaging_id IS NOT NULL
         GROUP BY drug_packaging_id
         UNION ALL
         SELECT 'service'::treatment_item_kind, service_id, count(*)
         FROM treatment_item WHERE service_id IS NOT NULL
         GROUP BY service_id",
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    transaction.commit().await?;
    Ok(inserted)
}

/// Deletes drafts that were opened, never filled in, and left behind — and only those:
/// every completeness field is still empty and nothing hangs off the row.
pub async fn cleanup_abandoned_drafts(pool: &PgPool) -> AppResult<DraftCleanup> {
    let cutoff = TimeDelta::try_hours(DRAFT_MAX_AGE_HOURS).unwrap_or_default();
    let cutoff = chrono::Utc::now() - cutoff;
    let mut removed = DraftCleanup::default();
    let mut transaction = pool.begin().await?;

    removed.patients = sqlx::query!(
        "DELETE FROM patient
         WHERE draft AND created_at < $1
           AND customer_id IS NULL AND name IS NULL AND sex IS NULL AND species IS NULL
           AND NOT EXISTS (SELECT 1 FROM treatment_patient WHERE patient_id = patient.id)
           AND NOT EXISTS (SELECT 1 FROM attachment WHERE patient_id = patient.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.customers = sqlx::query!(
        "DELETE FROM customer
         WHERE draft AND created_at < $1
           AND salutation IS NULL AND first_name IS NULL AND last_name IS NULL
           AND home_street IS NULL AND home_zip IS NULL AND home_city IS NULL
           AND NOT EXISTS (SELECT 1 FROM patient WHERE customer_id = customer.id)
           AND NOT EXISTS (SELECT 1 FROM customer_email WHERE customer_id = customer.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.packagings = sqlx::query!(
        "DELETE FROM drug_packaging
         WHERE draft AND created_at < $1
           AND unit IS NULL AND quantity IS NULL AND list_price_net IS NULL
           AND sales_price_gross IS NULL AND supplier_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM drug_stock_lot WHERE packaging_id = drug_packaging.id)
           AND NOT EXISTS (
                 SELECT 1 FROM treatment_item WHERE drug_packaging_id = drug_packaging.id)
           AND NOT EXISTS (
                 SELECT 1 FROM treatment_template_item
                 WHERE drug_packaging_id = drug_packaging.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.drugs = sqlx::query!(
        "DELETE FROM drug
         WHERE draft AND created_at < $1
           AND name IS NULL AND manufacturer_id IS NULL AND vat_percent IS NULL
           AND NOT EXISTS (SELECT 1 FROM drug_packaging WHERE drug_id = drug.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.suppliers = sqlx::query!(
        "DELETE FROM supplier
         WHERE draft AND created_at < $1 AND name IS NULL
           AND addr_street IS NULL AND addr_zip IS NULL AND addr_city IS NULL
           AND NOT EXISTS (SELECT 1 FROM drug_packaging WHERE supplier_id = supplier.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.manufacturers = sqlx::query!(
        "DELETE FROM manufacturer
         WHERE draft AND created_at < $1 AND name IS NULL
           AND addr_street IS NULL AND addr_zip IS NULL AND addr_city IS NULL
           AND NOT EXISTS (SELECT 1 FROM drug WHERE manufacturer_id = manufacturer.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.services = sqlx::query!(
        "DELETE FROM service
         WHERE draft AND created_at < $1
           AND name IS NULL AND vat_percent IS NULL AND gross_price IS NULL
           AND got_number IS NULL
           AND NOT EXISTS (SELECT 1 FROM treatment_item WHERE service_id = service.id)
           AND NOT EXISTS (
                 SELECT 1 FROM treatment_template_item WHERE service_id = service.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.templates = sqlx::query!(
        "DELETE FROM treatment_template
         WHERE draft AND created_at < $1 AND name IS NULL
           AND NOT EXISTS (
                 SELECT 1 FROM treatment_template_item
                 WHERE template_id = treatment_template.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    removed.appointments = sqlx::query!(
        "DELETE FROM appointment
         WHERE draft AND created_at < $1 AND starts_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM treatment WHERE appointment_id = appointment.id)",
        cutoff,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    transaction.commit().await?;
    Ok(removed)
}

/// Removes attachments nothing points at any more, with their files. Returns the count.
///
/// Only `referenced` uploads can become orphans: patient and treatment files are deleted
/// with their owner by the foreign keys.
pub async fn sweep_orphan_attachments(pool: &PgPool, files: &AttachmentStore) -> AppResult<u64> {
    let cutoff = TimeDelta::try_days(ORPHAN_MAX_AGE_DAYS).unwrap_or_default();
    let cutoff = chrono::Utc::now() - cutoff;

    let orphans = sqlx::query!(
        "DELETE FROM attachment
         WHERE kind = 'referenced' AND created_at < $1
           AND patient_id IS NULL AND treatment_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM patient WHERE photo_attachment_id = attachment.id)
           AND NOT EXISTS (SELECT 1 FROM invoice WHERE pdf_attachment_id = attachment.id)
           AND NOT EXISTS (
                 SELECT 1 FROM global_settings WHERE logo_attachment_id = attachment.id)
         RETURNING sha256",
        cutoff,
    )
    .fetch_all(pool)
    .await?;

    // The file is only deleted once no row shares its hash — uploads are deduplicated.
    for orphan in &orphans {
        let still_used = sqlx::query_scalar!(
            "SELECT true FROM attachment WHERE sha256 = $1 LIMIT 1",
            orphan.sha256
        )
        .fetch_optional(pool)
        .await?;
        if still_used.is_none() {
            files.remove(&orphan.sha256).await?;
        }
    }

    Ok(orphans.len() as u64)
}

/// Runs the nightly chores at 03:00 local time, and the orphan sweep on the first of the
/// month. A failing job logs and leaves the schedule intact.
pub fn spawn(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let wait = until_next_run();
            tracing::info!(seconds = wait.as_secs(), "maintenance sleeping until 03:00");
            tokio::time::sleep(wait).await;

            match refresh_picker_usage(&state.pool).await {
                Ok(rows) => tracing::info!(rows, "picker usage refreshed"),
                Err(error) => tracing::error!(%error, "refreshing the picker usage failed"),
            }
            match cleanup_abandoned_drafts(&state.pool).await {
                Ok(removed) if removed.total() > 0 => {
                    tracing::info!(?removed, "abandoned drafts removed");
                }
                Ok(_) => tracing::debug!("no abandoned drafts"),
                Err(error) => tracing::error!(%error, "cleaning up drafts failed"),
            }

            if Local::now().day() == 1 {
                match sweep_orphan_attachments(&state.pool, &state.files).await {
                    Ok(count) => tracing::info!(count, "orphaned attachments swept"),
                    Err(error) => tracing::error!(%error, "sweeping attachments failed"),
                }
            }
        }
    })
}

/// Time until the next 03:00 local time, at least a minute away so a run at 03:00 sharp
/// cannot loop.
fn until_next_run() -> Duration {
    let now = Local::now();
    let today = NaiveTime::from_hms_opt(NIGHTLY_HOUR, 0, 0).unwrap_or_default();
    let mut next = now.with_time(today).earliest().unwrap_or(now);
    if next <= now + TimeDelta::try_minutes(1).unwrap_or_default() {
        next += TimeDelta::try_days(1).unwrap_or_default();
    }
    (next - now).to_std().unwrap_or(Duration::from_secs(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_run_is_always_in_the_future_and_within_a_day() {
        let wait = until_next_run();
        assert!(wait.as_secs() >= 60, "never fires twice in one night");
        assert!(
            wait.as_secs() <= 24 * 60 * 60,
            "at most one day out: {wait:?}"
        );
    }
}
