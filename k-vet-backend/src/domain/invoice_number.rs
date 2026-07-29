//! Invoice number pattern, counter scope and rendering (research R18).
//!
//! The operator configures a pattern such as `{year}-{counter:4}`. The counter's scope is
//! derived from the pattern's date parts, so "does the counter reset yearly?" is answered
//! by the pattern itself instead of a second setting. Numbers are never reused: the
//! sequence row is monotonic and `UNIQUE (invoice_number)` is the backstop.

use chrono::{Datelike, NaiveDate};
use sqlx::PgExecutor;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Year,
    Month,
    Counter { width: usize },
}

/// A validated invoice number pattern.
#[derive(Debug, Clone)]
pub struct NumberPattern {
    segments: Vec<Segment>,
    has_year: bool,
    has_month: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    #[error("the invoice number pattern must contain {{counter}}")]
    MissingCounter,
    #[error("the invoice number pattern must contain {{counter}} exactly once")]
    DuplicateCounter,
    #[error("unknown placeholder `{{{0}}}` in the invoice number pattern")]
    UnknownPlaceholder(String),
    #[error("unterminated placeholder in the invoice number pattern")]
    UnterminatedPlaceholder,
    #[error("invalid counter width `{0}` in the invoice number pattern")]
    InvalidCounterWidth(String),
}

/// Parses and validates the configured pattern — called at startup so a typo is a boot
/// error rather than a surprise at invoicing time.
pub fn validate_pattern(pattern: &str) -> Result<NumberPattern, PatternError> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut counters = 0usize;
    let mut has_year = false;
    let mut has_month = false;
    let mut rest = pattern;

    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or(PatternError::UnterminatedPlaceholder)?;
        let name = &after[..close];

        if !literal.is_empty() {
            segments.push(Segment::Literal(std::mem::take(&mut literal)));
        }
        match name {
            "year" => {
                has_year = true;
                segments.push(Segment::Year);
            }
            "month" => {
                has_month = true;
                segments.push(Segment::Month);
            }
            "counter" => {
                counters += 1;
                segments.push(Segment::Counter { width: 1 });
            }
            other => {
                let width = other
                    .strip_prefix("counter:")
                    .ok_or_else(|| PatternError::UnknownPlaceholder(other.to_owned()))?;
                let width: usize = width
                    .parse()
                    .map_err(|_| PatternError::InvalidCounterWidth(width.to_owned()))?;
                counters += 1;
                segments.push(Segment::Counter { width });
            }
        }
        rest = &after[close + 1..];
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        segments.push(Segment::Literal(literal));
    }

    match counters {
        0 => Err(PatternError::MissingCounter),
        1 => Ok(NumberPattern {
            segments,
            has_year,
            has_month,
        }),
        _ => Err(PatternError::DuplicateCounter),
    }
}

impl NumberPattern {
    /// The counter scope for an invoice date — the pattern's date parts, rendered.
    pub fn scope(&self, invoice_date: NaiveDate) -> String {
        match (self.has_year, self.has_month) {
            (true, true) => format!("{:04}-{:02}", invoice_date.year(), invoice_date.month()),
            (true, false) => format!("{:04}", invoice_date.year()),
            (false, true) => format!("{:02}", invoice_date.month()),
            (false, false) => String::new(),
        }
    }

    /// Renders the invoice number for a date and an allocated counter value.
    pub fn render(&self, invoice_date: NaiveDate, counter: i64) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Year => out.push_str(&format!("{:04}", invoice_date.year())),
                Segment::Month => out.push_str(&format!("{:02}", invoice_date.month())),
                Segment::Counter { width } => {
                    out.push_str(&format!("{counter:0width$}", width = *width));
                }
            }
        }
        out
    }
}

/// Allocates the next number for `invoice_date`: one atomic upsert on the scope's counter
/// row, then rendering. Monotonic — the counter is never decremented, so a cancelled
/// invoice's number stays burned.
pub async fn allocate_number<'e, E>(
    executor: E,
    pattern: &NumberPattern,
    invoice_date: NaiveDate,
) -> AppResult<String>
where
    E: PgExecutor<'e>,
{
    let scope = pattern.scope(invoice_date);
    let counter: i64 = sqlx::query_scalar!(
        "INSERT INTO invoice_number_sequence (scope, counter)
         VALUES ($1, 1)
         ON CONFLICT (scope) DO UPDATE SET counter = invoice_number_sequence.counter + 1
         RETURNING counter",
        scope,
    )
    .fetch_one(executor)
    .await
    .map_err(AppError::Database)?;

    Ok(pattern.render(invoice_date, counter))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid test date")
    }

    #[test]
    fn renders_year_and_zero_padded_counter() {
        let pattern = validate_pattern("{year}-{counter:4}").expect("valid pattern");
        assert_eq!(pattern.render(date(2026, 3, 14), 42), "2026-0042");
        assert_eq!(pattern.render(date(2026, 3, 14), 1), "2026-0001");
        assert_eq!(
            pattern.render(date(2026, 3, 14), 12345),
            "2026-12345",
            "never truncates"
        );
    }

    #[test]
    fn renders_literals_month_and_unpadded_counter() {
        let pattern = validate_pattern("R-{year}/{month}/{counter:5}").expect("valid pattern");
        assert_eq!(pattern.render(date(2026, 3, 14), 42), "R-2026/03/00042");

        let plain = validate_pattern("RG{counter}").expect("valid pattern");
        assert_eq!(plain.render(date(2026, 3, 14), 7), "RG7");
    }

    #[test]
    fn scope_follows_the_patterns_date_parts() {
        let yearly = validate_pattern("{year}-{counter:4}").expect("valid pattern");
        assert_eq!(yearly.scope(date(2026, 3, 14)), "2026");
        assert_eq!(
            yearly.scope(date(2027, 1, 1)),
            "2027",
            "a new year starts a new counter"
        );

        let monthly = validate_pattern("{year}{month}-{counter:3}").expect("valid pattern");
        assert_eq!(monthly.scope(date(2026, 3, 14)), "2026-03");
        assert_eq!(monthly.scope(date(2026, 4, 1)), "2026-04");

        let global = validate_pattern("RG-{counter:6}").expect("valid pattern");
        assert_eq!(
            global.scope(date(2026, 3, 14)),
            "",
            "no date part means one counter"
        );
    }

    #[test]
    fn month_only_patterns_scope_by_month() {
        let pattern = validate_pattern("{month}/{counter:3}").expect("valid pattern");
        assert_eq!(pattern.scope(date(2026, 3, 14)), "03");
    }

    #[test]
    fn pattern_must_contain_a_counter() {
        let error = validate_pattern("{year}-noise").expect_err("a pattern without a counter");
        assert!(matches!(error, PatternError::MissingCounter));
    }

    #[test]
    fn unknown_placeholders_are_rejected_at_startup() {
        let error =
            validate_pattern("{year}-{customer}-{counter}").expect_err("unknown placeholder");
        match error {
            PatternError::UnknownPlaceholder(name) => assert_eq!(name, "customer"),
            other => panic!("expected an unknown-placeholder error, got {other:?}"),
        }
    }

    #[test]
    fn unbalanced_braces_are_rejected() {
        assert!(matches!(
            validate_pattern("{year-{counter}"),
            Err(PatternError::UnknownPlaceholder(_))
        ));
        assert!(matches!(
            validate_pattern("{year}-{counter"),
            Err(PatternError::UnterminatedPlaceholder)
        ));
    }

    #[test]
    fn counter_width_must_be_a_number() {
        assert!(matches!(
            validate_pattern("{year}-{counter:abc}"),
            Err(PatternError::InvalidCounterWidth(_))
        ));
    }

    #[test]
    fn two_counters_are_rejected() {
        assert!(matches!(
            validate_pattern("{counter}-{counter}"),
            Err(PatternError::DuplicateCounter)
        ));
    }
}
