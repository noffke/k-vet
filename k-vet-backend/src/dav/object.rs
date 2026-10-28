//! What both collections share: an object and its ETag, the collection's ctag, and the
//! conditions and outcomes of a write from the phone.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::error::AppError;

/// One file of a collection: an event, or a contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// The last segment of its href: `kvet-12.ics`, `kvet-customer-3.vcf`, or the phone's own name.
    pub name: String,
    /// Quoted, as the `ETag` header carries it.
    pub etag: String,
    /// The iCalendar or vCard text.
    pub body: String,
    /// The event's start, for a `calendar-query` time range; absent for kept entries.
    pub starts_at: Option<DateTime<Utc>>,
}

/// Quoted SHA-256 of the text — the same text always gives the same ETag.
pub fn etag_of(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// A hash over every object's name and ETag: changes whenever anything in the collection does.
pub fn ctag(objects: &[Object]) -> String {
    let joined: String = objects
        .iter()
        .map(|object| format!("{}={};", object.name, object.etag))
        .collect();
    etag_of(&joined)
}

/// Why a write from the phone is not taken.
#[derive(Debug)]
pub enum WriteError {
    /// `If-Match` / `If-None-Match` do not hold: the phone has an older or newer copy.
    Precondition,
    NotFound,
    /// The body cannot be read; the reason is for the log.
    Unreadable(&'static str),
    App(AppError),
}

impl From<AppError> for WriteError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<sqlx::Error> for WriteError {
    fn from(error: sqlx::Error) -> Self {
        Self::App(error.into())
    }
}

/// The `If-Match` / `If-None-Match` a write came with.
#[derive(Debug, Default, Clone)]
pub struct Conditions {
    pub if_match: Option<String>,
    pub if_none_match: Option<String>,
}

impl Conditions {
    pub fn hold(&self, current: Option<&Object>) -> bool {
        if let Some(expected) = &self.if_match {
            let matches = current.is_some_and(|object| {
                expected.trim() == "*"
                    || expected
                        .split(',')
                        .any(|candidate| candidate.trim() == object.etag)
            });
            if !matches {
                return false;
            }
        }
        if let Some(none) = &self.if_none_match
            && current.is_some()
            && (none.trim() == "*"
                || none
                    .split(',')
                    .any(|candidate| current.is_some_and(|object| candidate.trim() == object.etag)))
        {
            return false;
        }
        true
    }
}

/// Whether the PUT made something new (201) or changed what was there (204).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    Created,
    Updated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_compare_etags() {
        let current = Object {
            name: "kvet-1.ics".to_owned(),
            etag: "\"abc\"".to_owned(),
            body: String::new(),
            starts_at: None,
        };
        let none = Conditions::default();
        assert!(none.hold(Some(&current)) && none.hold(None));
        let create = Conditions {
            if_none_match: Some("*".to_owned()),
            ..Conditions::default()
        };
        assert!(create.hold(None));
        assert!(!create.hold(Some(&current)));
        let update = Conditions {
            if_match: Some("\"abc\"".to_owned()),
            ..Conditions::default()
        };
        assert!(update.hold(Some(&current)));
        let stale = Conditions {
            if_match: Some("\"old\"".to_owned()),
            ..Conditions::default()
        };
        assert!(!stale.hold(Some(&current)));
        assert!(!update.hold(None));
    }
}
