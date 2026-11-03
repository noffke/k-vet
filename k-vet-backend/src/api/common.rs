//! Shared request shapes for the resource modules.

use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppResult};

/// Default page size for list endpoints; the practice's lists are small.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

/// Query parameters every list endpoint understands.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Text filter (name, number, …).
    pub q: Option<String>,
    /// Include archived records (default: false).
    pub archived: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl ListQuery {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }

    pub fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }

    pub fn include_archived(&self) -> bool {
        self.archived.unwrap_or(false)
    }

    /// Trimmed search term, or `None` when the filter is empty.
    pub fn search(&self) -> Option<String> {
        self.q
            .as_ref()
            .map(|term| term.trim().to_owned())
            .filter(|term| !term.is_empty())
    }
}

/// Only an incomplete record can be deleted — the one clicked into existence by mistake. A
/// complete one is archived, never deleted (`requirements/datamodel.md`). `draft` is the row's
/// flag as read `FOR UPDATE`, `None` when there is no such row.
pub fn ensure_deletable(draft: Option<bool>) -> AppResult<()> {
    match draft {
        None => Err(AppError::NotFound),
        Some(false) => Err(AppError::Conflict(
            "a complete record is archived, not deleted".to_owned(),
        )),
        Some(true) => Ok(()),
    }
}

/// A delete refused by a foreign key: something still refers to the record. That is a 409 the
/// screen can name, not the 422 "referenced record does not exist" an insert would earn.
pub fn in_use(error: sqlx::Error) -> AppError {
    let refused = error
        .as_database_error()
        .is_some_and(|db| matches!(db.kind(), sqlx::error::ErrorKind::ForeignKeyViolation));
    if refused {
        AppError::Conflict("still in use".to_owned())
    } else {
        AppError::Database(error)
    }
}

/// Distinguishes "field absent" from "field explicitly set to null" in PATCH bodies.
///
/// `None` — leave the column alone. `Some(None)` — clear it. `Some(Some(v))` — set it.
pub fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// How a duplicate treats the prices pinned on the source's lines (FR-026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriceMode {
    /// Copy the stored values exactly as they are.
    #[default]
    Verbatim,
    /// Re-read names and prices from the current catalog.
    Refresh,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct DuplicateRequest {
    #[serde(default)]
    pub price_mode: PriceMode,
}

/// `{direction}` body of the reorder endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveDirection {
    Up,
    Down,
    Top,
    Bottom,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveRequest {
    pub direction: MoveDirection,
}
