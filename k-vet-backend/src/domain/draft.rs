//! Draft-row completeness (research R15).
//!
//! Form-edited entities are created immediately as `draft = true` so auto-save has a row
//! to PATCH. The server recomputes the flag on every write: it flips to `false` with the
//! write that fills the last mandatory field, and clearing a mandatory field on a complete
//! row is rejected — the DB's `CHECK (draft OR …)` is the backstop, this module is the
//! friendly error message in front of it.

use crate::error::{AppError, AppResult, FieldError};

/// A mandatory field and whether the row currently has a value for it.
pub type FieldPresence = (&'static str, bool);

/// Names of the mandatory fields that are still empty, in declaration order.
pub fn missing_fields(fields: &[FieldPresence]) -> Vec<&'static str> {
    fields
        .iter()
        .filter(|(_, present)| !present)
        .map(|(name, _)| *name)
        .collect()
}

/// `true` while at least one mandatory field is empty.
pub fn is_draft(fields: &[FieldPresence]) -> bool {
    fields.iter().any(|(_, present)| !present)
}

/// Recomputes the draft flag and enforces the one-way transition.
///
/// * a draft row stays a draft until every mandatory field is filled
/// * a complete row may never fall back to draft: clearing a mandatory field yields a
///   422 with field-level errors, and the caller must keep the last valid value
pub fn recompute_draft(was_draft: bool, fields: &[FieldPresence]) -> AppResult<bool> {
    let missing = missing_fields(fields);
    if missing.is_empty() {
        return Ok(false);
    }
    if was_draft {
        return Ok(true);
    }
    Err(AppError::Validation(
        missing
            .into_iter()
            .map(|field| FieldError::new(field, "field.required"))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLETE: &[FieldPresence] = &[
        ("salutation", true),
        ("last_name", true),
        ("home_street", true),
    ];
    const PARTIAL: &[FieldPresence] = &[
        ("salutation", true),
        ("last_name", false),
        ("home_street", false),
    ];

    #[test]
    fn reports_missing_fields_in_order() {
        assert_eq!(missing_fields(PARTIAL), vec!["last_name", "home_street"]);
        assert!(missing_fields(COMPLETE).is_empty());
    }

    #[test]
    fn draft_flag_follows_completeness() {
        assert!(is_draft(PARTIAL));
        assert!(!is_draft(COMPLETE));
    }

    #[test]
    fn completing_the_last_field_leaves_draft_state() {
        let draft = recompute_draft(true, COMPLETE).expect("complete row is accepted");
        assert!(!draft);
    }

    #[test]
    fn incomplete_draft_stays_draft() {
        let draft = recompute_draft(true, PARTIAL).expect("draft rows may be incomplete");
        assert!(draft);
    }

    #[test]
    fn clearing_a_mandatory_field_on_a_complete_row_is_rejected() {
        let error = recompute_draft(false, PARTIAL).expect_err("one-way transition");
        match error {
            AppError::Validation(errors) => {
                assert_eq!(errors.len(), 2);
                assert_eq!(errors[0].field, "last_name");
                assert_eq!(errors[0].message, "field.required");
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }
}
