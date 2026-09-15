//! Application error type and its RFC 7807 `application/problem+json` representation.
//!
//! Database CHECK constraints and partial unique indexes are first-class validation
//! (Constitution IV), so violations are translated into the same field-level 422/409
//! responses the handlers produce themselves.

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

/// One field-level validation failure, as returned in `problem+json`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FieldError {
    /// Name of the offending field, in wire format (e.g. `home_zip`).
    pub field: String,
    /// Human readable, translatable-by-key message.
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// RFC 7807 problem details.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    /// URI reference identifying the problem type.
    pub r#type: String,
    /// Short, human readable summary.
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Explanation specific to this occurrence.
    pub detail: String,
    /// Field-level errors — present for validation failures.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("resource not found")]
    NotFound,
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("i/o error")]
    Io(#[from] std::io::Error),
    /// The mail server could not be reached, or refused the message.
    ///
    /// Distinct from [`Self::Internal`] because it is the one server-side failure the vet can
    /// do something about — and the one they meet regularly, since an appliance on a practice
    /// network reaches its SMTP relay or does not. The detail is a translation key rather than
    /// the transport's own words, which are English and name hosts and ports.
    #[error("the mail server could not be reached")]
    MailTransport,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// A single-field validation failure.
    pub fn field(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation(vec![FieldError::new(field, message)])
    }

    /// Wraps any error with context for the log line; the client only sees a 500.
    pub fn internal(context: impl std::fmt::Display, source: impl std::fmt::Display) -> Self {
        Self::Internal(format!("{context}: {source}"))
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Database(error) => match database_problem(error) {
                Some(problem) => problem.status,
                None => StatusCode::INTERNAL_SERVER_ERROR,
            },
            // Upstream, not us: the practice's relay is the thing that did not answer.
            Self::MailTransport => StatusCode::BAD_GATEWAY,
            Self::Io(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn problem(&self) -> ProblemDetails {
        let status = self.status();
        let (detail, errors) = match self {
            Self::Validation(errors) => ("validation failed".to_owned(), errors.clone()),
            Self::Database(error) => match database_problem(error) {
                Some(problem) => (problem.detail, problem.errors),
                None => ("internal server error".to_owned(), Vec::new()),
            },
            Self::Io(_) | Self::Internal(_) => ("internal server error".to_owned(), Vec::new()),
            // A key, not prose: this one reaches the screen, and the screen is German.
            Self::MailTransport => ("invoice.mailUnreachable".to_owned(), Vec::new()),
            other => (other.to_string(), Vec::new()),
        };
        ProblemDetails {
            r#type: "about:blank".to_owned(),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status: status.as_u16(),
            detail,
            errors,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Errors are logged with context where they surface; the client never sees internals.
        if status.is_server_error() {
            tracing::error!(error = %self, status = %status, "request failed");
        } else {
            tracing::debug!(error = %self, status = %status, "request rejected");
        }
        let problem = self.problem();
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(problem),
        )
            .into_response()
    }
}

struct DatabaseProblem {
    status: StatusCode,
    detail: String,
    errors: Vec<FieldError>,
}

/// Translates a database error into a client-facing problem, or `None` for a genuine 500.
fn database_problem(error: &sqlx::Error) -> Option<DatabaseProblem> {
    if matches!(error, sqlx::Error::RowNotFound) {
        return Some(DatabaseProblem {
            status: StatusCode::NOT_FOUND,
            detail: "resource not found".to_owned(),
            errors: Vec::new(),
        });
    }
    let db_error = error.as_database_error()?;
    let constraint = db_error.constraint().unwrap_or_default().to_owned();
    let kind = db_error.kind();

    if let Some((field, message)) = constraint_field_message(&constraint) {
        return Some(DatabaseProblem {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            detail: "validation failed".to_owned(),
            errors: vec![FieldError::new(field, message)],
        });
    }

    match kind {
        sqlx::error::ErrorKind::UniqueViolation => Some(DatabaseProblem {
            status: StatusCode::CONFLICT,
            detail: format!("conflicting value ({constraint})"),
            errors: Vec::new(),
        }),
        sqlx::error::ErrorKind::ForeignKeyViolation => Some(DatabaseProblem {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            detail: format!("referenced record does not exist ({constraint})"),
            errors: Vec::new(),
        }),
        sqlx::error::ErrorKind::CheckViolation | sqlx::error::ErrorKind::NotNullViolation => {
            Some(DatabaseProblem {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                detail: format!("value rejected by the database ({constraint})"),
                errors: Vec::new(),
            })
        }
        _ => None,
    }
}

/// Maps the constraint names the domain relies on to the field the user must fix.
/// Message keys are resolved by the frontend's i18n layer.
fn constraint_field_message(constraint: &str) -> Option<(&'static str, &'static str)> {
    let mapped = match constraint {
        "customer_complete" => ("draft", "record.incomplete"),
        "patient_complete" => ("draft", "record.incomplete"),
        "patient_sex_values" => ("sex", "patient.sex.invalid"),
        "drug_complete" => ("draft", "record.incomplete"),
        "drug_packaging_complete" => ("draft", "record.incomplete"),
        "drug_packaging_quantity_positive" => ("quantity", "value.mustBePositive"),
        "drug_packaging_supplier_kind" => ("supplier_id", "packaging.supplierOnOriginalOnly"),
        "drug_packaging_one_original_idx" => ("kind", "packaging.originalAlreadyExists"),
        "service_complete" => ("draft", "record.incomplete"),
        "service_got_shape" => ("got_number", "service.gotFieldsRequired"),
        "service_factor_positive" => ("factor", "value.mustBePositive"),
        "supplier_complete" | "manufacturer_complete" => ("name", "record.incomplete"),
        "treatment_template_complete" => ("name", "record.incomplete"),
        "appointment_complete" => ("starts_at", "record.incomplete"),
        "treatment_item_quantity_positive" => ("quantity", "value.mustBePositive"),
        "treatment_item_drug_needs_patient" => ("patient_id", "item.patientRequired"),
        "treatment_item_drug_shape" => ("kind", "item.drugShapeInvalid"),
        "treatment_item_km_shape" => ("km", "item.kmInvalid"),
        "drug_stock_lot_packages_positive" => ("packages_received", "value.mustBePositive"),
        "drug_stock_lot_original_only" => ("packaging_id", "lot.originalPackagingOnly"),
        "drug_stock_movement_dispense_shape" => ("quantity", "movement.dispenseShapeInvalid"),
        "drug_stock_movement_correction_shape" => ("reason", "movement.correctionShapeInvalid"),
        "drug_stock_movement_quantity_nonzero" => ("quantity", "value.mustNotBeZero"),
        // The trigger that keeps a lot's derived stock at or above zero (FR-018). It is the
        // backstop under every write path, including ones that never go through a handler.
        "drug_stock_movement_lot_not_negative" => ("new_remaining", "movement.wouldGoNegative"),
        "invoice_one_live_per_treatment_idx" => ("treatment_id", "invoice.liveInvoiceExists"),
        _ => return None,
    };
    Some(mapped)
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_reports_fields() {
        let error = AppError::field("home_zip", "field.required");
        let problem = error.problem();
        assert_eq!(problem.status, 422);
        assert_eq!(problem.errors.len(), 1);
        assert_eq!(problem.errors[0].field, "home_zip");
    }

    #[test]
    fn not_found_maps_to_404() {
        assert_eq!(AppError::NotFound.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn row_not_found_maps_to_404() {
        let error = AppError::Database(sqlx::Error::RowNotFound);
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn internal_errors_hide_their_detail() {
        let error = AppError::internal("rendering invoice", "typst exploded");
        let problem = error.problem();
        assert_eq!(problem.status, 500);
        assert_eq!(problem.detail, "internal server error");
    }

    #[test]
    fn known_constraints_map_to_fields() {
        let mapped = constraint_field_message("treatment_item_drug_needs_patient");
        assert_eq!(mapped, Some(("patient_id", "item.patientRequired")));
        assert_eq!(constraint_field_message("no_such_constraint"), None);
    }
}
