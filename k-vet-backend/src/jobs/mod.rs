//! Scheduled maintenance (T076), and the mail queue.
//!
//! Three chores the practice should never have to think about:
//!   * the picker's usage weights, rebuilt nightly from what was actually billed
//!   * drafts that were opened by accident and never filled in, dropped after a day
//!   * uploaded files nothing points at any more, swept once a month
//!
//! And every five minutes, the invoice mail that is waiting to go out (`api::mail_queue`).
//!
//! Each job is a plain async function so the tests can run it directly; `spawn` and
//! `spawn_mail_queue` are the only parts that deal with the clock.

use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, TimeDelta};
use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::AppState;
use crate::api::{invoices, mail_queue};
use crate::domain::enums::InvoiceStatus;
use crate::domain::files::AttachmentStore;
use crate::error::AppResult;

/// Nightly run time — after surgery hours, before the morning backup.
const NIGHTLY_HOUR: u32 = 3;
/// A draft the vet never typed into is abandoned after this long.
const DRAFT_MAX_AGE_HOURS: i64 = 24;
/// An upload nothing references is only swept once it is clearly not in flight.
const ORPHAN_MAX_AGE_DAYS: i64 = 30;
/// How often the mail queue is worked through.
const MAIL_QUEUE_INTERVAL: Duration = Duration::from_secs(5 * 60);

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
           AND NOT EXISTS (SELECT 1 FROM patient_treatment WHERE patient_id = patient.id)
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
           AND sales_price_net IS NULL AND supplier_id IS NULL
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
           AND name IS NULL AND vat_percent IS NULL AND net_price IS NULL
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

    // An appointment always has its treatment (migration 0022), so "abandoned" means that
    // treatment is still empty: no animal, no line, no invoice. The delete takes it along.
    removed.appointments = sqlx::query!(
        "DELETE FROM appointment
         WHERE draft AND created_at < $1 AND starts_at IS NULL
           AND NOT EXISTS (
                 SELECT 1 FROM treatment
                 WHERE treatment.appointment_id = appointment.id
                   AND (EXISTS (SELECT 1 FROM patient_treatment
                                WHERE patient_treatment.treatment_id = treatment.id)
                        OR EXISTS (SELECT 1 FROM treatment_item
                                   WHERE treatment_item.treatment_id = treatment.id)
                        OR EXISTS (SELECT 1 FROM invoice
                                   WHERE invoice.treatment_id = treatment.id)))",
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
           AND patient_id IS NULL AND patient_treatment_id IS NULL
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

/// What a run of the mail queue did — logged, and what the tests read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MailRun {
    pub sent: u64,
    /// Failed, and will be tried again.
    pub failed: u64,
    /// Failed for the last time: refused, or still failing six hours after the first failure.
    pub gave_up: u64,
    /// Waiting for an invoice that was cancelled meanwhile — dropped, never sent.
    pub dropped: u64,
}

/// Works through the mail queue once: every mail that has not been given up, oldest first.
///
/// Each is tried in a transaction of its own that locks the queue row and the invoice, so a
/// resend, a retry, a removal or a cancel arriving meanwhile waits for the attempt rather than
/// racing it. A delivered mail leaves the queue, and its delivery is recorded on the invoice in
/// the same transaction. A failure stays on the row with the transport's own words; it is tried
/// again on the next run until [`mail_queue::RETRY_HOURS`] after the first failure — unless it
/// was a refusal, which is final at once.
pub async fn send_queued_mail(state: &AppState) -> AppResult<MailRun> {
    let waiting = sqlx::query_scalar!(
        "SELECT id FROM mail_queue WHERE gave_up_at IS NULL ORDER BY queued_at, id"
    )
    .fetch_all(&state.pool)
    .await?;

    let mut run = MailRun::default();
    for id in waiting {
        let mut transaction = state.pool.begin().await?;
        let row = sqlx::query!(
            r#"SELECT queue.invoice_id, queue.recipients,
                      invoice.status AS "status: InvoiceStatus"
               FROM mail_queue queue
               JOIN invoice ON invoice.id = queue.invoice_id
               WHERE queue.id = $1 AND queue.gave_up_at IS NULL
               FOR UPDATE OF queue, invoice"#,
            id,
        )
        .fetch_optional(&mut *transaction)
        .await?;
        // Removed, or given up, since the list was read.
        let Some(row) = row else {
            continue;
        };

        if row.status == InvoiceStatus::Cancelled {
            // Cancelling drops the queued mail itself; this only guards against a row that
            // slipped past it.
            sqlx::query!("DELETE FROM mail_queue WHERE id = $1", id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            run.dropped += 1;
            continue;
        }

        match invoices::deliver(state, &mut transaction, row.invoice_id, &row.recipients).await? {
            Ok(()) => {
                sqlx::query!("DELETE FROM mail_queue WHERE id = $1", id)
                    .execute(&mut *transaction)
                    .await?;
                run.sent += 1;
            }
            Err(failure) => {
                tracing::warn!(
                    invoice_id = row.invoice_id,
                    refused = failure.refused,
                    error = %failure.detail,
                    "an invoice mail did not go out"
                );
                // The right-hand side reads the row as it was, so `first_failure_at` here is
                // the earlier failure, if there was one.
                let gave_up = sqlx::query_scalar!(
                    r#"UPDATE mail_queue
                          SET attempts = attempts + 1, last_attempt_at = now(),
                              last_error = $2, refused = $3,
                              first_failure_at = COALESCE(first_failure_at, now()),
                              gave_up_at = CASE
                                  WHEN $3 OR first_failure_at <= now() - make_interval(hours => $4)
                                  THEN now() END
                        WHERE id = $1
                       RETURNING gave_up_at IS NOT NULL AS "gave_up!""#,
                    id,
                    failure.detail,
                    failure.refused,
                    mail_queue::RETRY_HOURS,
                )
                .fetch_one(&mut *transaction)
                .await?;
                if gave_up {
                    run.gave_up += 1;
                } else {
                    run.failed += 1;
                }
            }
        }
        transaction.commit().await?;
    }
    Ok(run)
}

/// Works through the mail queue every five minutes, the first time right at start-up. A run
/// that fails logs and leaves the schedule intact.
///
/// Shutting down aborts the task, also in the middle of a send: the attempt's transaction rolls
/// back, so a mail the server had already taken would go out once more on the next start — the
/// price of never losing one.
pub fn spawn_mail_queue(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(MAIL_QUEUE_INTERVAL);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            match send_queued_mail(&state).await {
                Ok(run) if run == MailRun::default() => tracing::debug!("no mail waiting"),
                Ok(run) => tracing::info!(?run, "mail queue worked through"),
                Err(error) => tracing::error!(%error, "working through the mail queue failed"),
            }
        }
    })
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
