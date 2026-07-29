//! Email and phone validation (research R14).
//!
//! Both are pure functions so the rules are unit-tested against real-world German input.
//! The server is authoritative: invalid values are never persisted, the client only gets a
//! friendlier error earlier via the generated zod schemas.

use email_address::EmailAddress;
use phonenumber::country::Id as CountryId;

use crate::error::{AppError, AppResult};

/// Phone numbers without a country code are read as German.
const DEFAULT_REGION: CountryId = CountryId::DE;

/// Validates an email address, returning it trimmed.
pub fn validate_email(field: &str, email: &str) -> AppResult<String> {
    let trimmed = email.trim();
    if EmailAddress::is_valid(trimmed) {
        Ok(trimmed.to_owned())
    } else {
        Err(AppError::field(field.to_owned(), "value.invalidEmail"))
    }
}

/// A validated phone number in both the stored and the displayed form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneNumber {
    /// Storage form, e.g. `+493012345678`.
    pub e164: String,
    /// Display form for German numbers, e.g. `030 12345678`.
    pub national: String,
}

/// Parses "0171 / 123 45 67" and friends into E.164 plus a national display form.
pub fn validate_phone(field: &str, phone: &str) -> AppResult<PhoneNumber> {
    let trimmed = phone.trim();
    let parsed = phonenumber::parse(Some(DEFAULT_REGION), trimmed)
        .map_err(|_| AppError::field(field.to_owned(), "value.invalidPhone"))?;

    if !phonenumber::is_valid(&parsed) {
        return Err(AppError::field(field.to_owned(), "value.invalidPhone"));
    }

    Ok(PhoneNumber {
        e164: parsed.format().mode(phonenumber::Mode::E164).to_string(),
        national: parsed
            .format()
            .mode(phonenumber::Mode::National)
            .to_string(),
    })
}

/// Formats a stored E.164 number for display; falls back to the stored value.
pub fn display_phone(stored: &str) -> String {
    match phonenumber::parse(Some(DEFAULT_REGION), stored) {
        Ok(parsed) => parsed
            .format()
            .mode(phonenumber::Mode::National)
            .to_string(),
        Err(_) => stored.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_email_addresses() {
        assert_eq!(
            validate_email("email", " erika@example.com ").expect("valid"),
            "erika@example.com"
        );
        assert!(validate_email("email", "erika.mustermann+praxis@example.co.uk").is_ok());
    }

    #[test]
    fn rejects_malformed_email_addresses() {
        for candidate in ["erika", "erika@", "@example.com", "erika example.com", ""] {
            let error = validate_email("email", candidate)
                .expect_err(&format!("`{candidate}` must be rejected"));
            match error {
                AppError::Validation(errors) => {
                    assert_eq!(errors[0].field, "email");
                    assert_eq!(errors[0].message, "value.invalidEmail");
                }
                other => panic!("expected a field error, got {other:?}"),
            }
        }
    }

    #[test]
    fn parses_german_numbers_however_they_are_typed() {
        // The way a German customer dictates a mobile number.
        let mobile = validate_phone("phone", "0171 / 123 45 67").expect("valid");
        assert_eq!(mobile.e164, "+491711234567");
        assert!(
            mobile.national.starts_with("0171"),
            "national: {}",
            mobile.national
        );

        let landline = validate_phone("phone", "030 12345678").expect("valid");
        assert_eq!(landline.e164, "+493012345678");

        let spaced = validate_phone("phone", "(030) 1234-5678").expect("valid");
        assert_eq!(spaced.e164, "+493012345678");
    }

    #[test]
    fn accepts_international_input() {
        let austrian = validate_phone("phone", "+43 1 234567").expect("valid");
        assert_eq!(austrian.e164, "+431234567");
    }

    #[test]
    fn rejects_numbers_that_are_not_real() {
        for candidate in ["12", "abcdef", "0171", ""] {
            assert!(
                validate_phone("phone", candidate).is_err(),
                "`{candidate}` must be rejected"
            );
        }
    }

    #[test]
    fn stored_numbers_are_displayed_nationally() {
        assert!(display_phone("+493012345678").starts_with("030"));
        // Unparseable legacy data is shown as stored rather than hidden.
        assert_eq!(display_phone("unknown"), "unknown");
    }
}
