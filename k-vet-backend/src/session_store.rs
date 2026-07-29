//! Postgres-backed session store for tower-sessions (research R2).
//!
//! Sessions live in the database so a Pi reboot never logs the vet out mid-day. The table
//! is created by migration `0007_files_settings.sql`; there is no runtime DDL.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tower_sessions::cookie::time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{Error, Result};
use tower_sessions::{SessionStore, session_store};

/// tower-sessions speaks `time::OffsetDateTime`, the database layer speaks `chrono` — the
/// conversion happens here so the rest of the crate has a single date/time crate.
fn to_chrono(expiry: OffsetDateTime) -> DateTime<Utc> {
    DateTime::from_timestamp_nanos(i64::try_from(expiry.unix_timestamp_nanos()).unwrap_or(i64::MAX))
}

#[derive(Clone, Debug)]
pub struct PostgresSessionStore {
    pool: PgPool,
}

impl PostgresSessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Removes sessions whose expiry has passed — called by the nightly maintenance job.
    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM tower_sessions.session WHERE expiry_date < now()")
            .execute(&self.pool)
            .await
            .map_err(backend_error)?;
        Ok(result.rows_affected())
    }
}

fn backend_error(error: sqlx::Error) -> Error {
    Error::Backend(error.to_string())
}

#[async_trait]
impl SessionStore for PostgresSessionStore {
    async fn create(&self, record: &mut Record) -> Result<()> {
        loop {
            let data = encode(record)?;
            let inserted = sqlx::query(
                "INSERT INTO tower_sessions.session (id, data, expiry_date)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(record.id.to_string())
            .bind(data)
            .bind(to_chrono(record.expiry_date))
            .execute(&self.pool)
            .await
            .map_err(backend_error)?;

            if inserted.rows_affected() == 1 {
                return Ok(());
            }
            // Session id collision — mint a new one and retry (as the trait requires).
            record.id = Id::default();
        }
    }

    async fn save(&self, record: &Record) -> Result<()> {
        sqlx::query(
            "INSERT INTO tower_sessions.session (id, data, expiry_date)
             VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET data = $2, expiry_date = $3",
        )
        .bind(record.id.to_string())
        .bind(encode(record)?)
        .bind(to_chrono(record.expiry_date))
        .execute(&self.pool)
        .await
        .map_err(backend_error)?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        let row: Option<(Vec<u8>, DateTime<Utc>)> = sqlx::query_as(
            "SELECT data, expiry_date FROM tower_sessions.session
             WHERE id = $1 AND expiry_date > now()",
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_error)?;

        match row {
            Some((data, _)) => Ok(Some(decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn delete(&self, session_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM tower_sessions.session WHERE id = $1")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

fn encode(record: &Record) -> session_store::Result<Vec<u8>> {
    serde_json::to_vec(record).map_err(|error| Error::Encode(error.to_string()))
}

fn decode(data: &[u8]) -> session_store::Result<Record> {
    serde_json::from_slice(data).map_err(|error| Error::Decode(error.to_string()))
}
