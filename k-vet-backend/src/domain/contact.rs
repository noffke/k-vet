//! Email and phone validation (research R14).
//!
//! Both are pure functions so the rules are unit-tested against real-world German input.
//! The server is authoritative: invalid values are never persisted, the client only gets a
//! friendlier error earlier via the generated zod schemas.
//!
//! A phone number typed without a country code belongs to a *region*: the customer's country,
//! or the configured default when the customer has none. One with a `+` carries its own.

use email_address::EmailAddress;
use phonenumber::country::Id as CountryId;

use crate::error::{AppError, AppResult};

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
    /// National form, e.g. `030 12345678`.
    pub national: String,
}

/// The region a phone number is read in, from an ISO 3166-1 alpha-2 code. A code the phone
/// metadata does not know leaves none, and then only numbers with a `+` parse.
fn region(code: &str) -> Option<CountryId> {
    code.trim().to_uppercase().parse().ok()
}

/// Parses "0171 / 123 45 67" and friends into E.164 plus a national form. A number without a
/// country code belongs to `region` (ISO 3166-1 alpha-2): "512 345 678" is +48 512 345 678 for
/// a customer in Poland — and a different, German number for one in Germany.
pub fn validate_phone(field: &str, phone: &str, region_code: &str) -> AppResult<PhoneNumber> {
    let trimmed = phone.trim();
    let parsed = phonenumber::parse(region(region_code), trimmed)
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
///
/// Only a number of `national_in` is shown nationally (`030 12345678`) — the caller passes the
/// practice's country when it is also the customer's, and nothing otherwise. Every other number
/// keeps its country code (`+48 512 345 678`). The display form is what the edit field is
/// filled with, so this is what makes saving it back safe: a national form is read in exactly
/// the region it was formatted for, and an international one carries its own.
pub fn display_phone(stored: &str, national_in: Option<&str>) -> String {
    let Ok(parsed) = phonenumber::parse(None, stored) else {
        return stored.to_owned();
    };
    let national = national_in
        .and_then(region)
        .is_some_and(|home| parsed.country().id() == Some(home));
    let mode = if national {
        phonenumber::Mode::National
    } else {
        phonenumber::Mode::International
    };
    parsed.format().mode(mode).to_string()
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
        let mobile = validate_phone("phone", "0171 / 123 45 67", "DE").expect("valid");
        assert_eq!(mobile.e164, "+491711234567");
        assert!(
            mobile.national.starts_with("0171"),
            "national: {}",
            mobile.national
        );

        let landline = validate_phone("phone", "030 12345678", "DE").expect("valid");
        assert_eq!(landline.e164, "+493012345678");

        let spaced = validate_phone("phone", "(030) 1234-5678", "DE").expect("valid");
        assert_eq!(spaced.e164, "+493012345678");
    }

    #[test]
    fn accepts_international_input() {
        let austrian = validate_phone("phone", "+43 1 234567", "DE").expect("valid");
        assert_eq!(austrian.e164, "+431234567");
        // A country code wins over the customer's region.
        let german = validate_phone("phone", "+49 30 12345678", "PL").expect("valid");
        assert_eq!(german.e164, "+493012345678");
    }

    #[test]
    fn numbers_without_a_country_code_belong_to_the_customers_region() {
        let polish = validate_phone("phone", "512 345 678", "PL").expect("valid in Poland");
        assert_eq!(polish.e164, "+48512345678");
        let lowercase = validate_phone("phone", "0664 1234567", "at").expect("valid in Austria");
        assert_eq!(lowercase.e164, "+436641234567");
        // The same digits read in Germany are a valid German number — the misreading a
        // region-blind parser would store without complaint.
        let german = validate_phone("phone", "512 345 678", "DE").expect("valid in Germany");
        assert_eq!(german.e164, "+49512345678");
    }

    #[test]
    fn a_region_without_phone_metadata_needs_a_country_code() {
        // Antarctica is a valid ISO 3166-1 code, but has no numbering plan to read "030" in.
        assert!(validate_phone("phone", "030 12345678", "AQ").is_err());
        assert!(validate_phone("phone", "+49 30 12345678", "AQ").is_ok());
    }

    #[test]
    fn rejects_numbers_that_are_not_real() {
        for candidate in ["12", "abcdef", "0171", ""] {
            assert!(
                validate_phone("phone", candidate, "DE").is_err(),
                "`{candidate}` must be rejected"
            );
        }
    }

    #[test]
    fn home_numbers_are_displayed_nationally_and_all_others_with_their_country_code() {
        assert_eq!(display_phone("+493012345678", Some("DE")), "030 12345678");
        assert_eq!(display_phone("+48512345678", Some("DE")), "+48 512 345 678");
        // A customer abroad: even a German number keeps its code.
        assert_eq!(display_phone("+493012345678", None), "+49 30 12345678");
        // Unparseable legacy data is shown as stored rather than hidden.
        assert_eq!(display_phone("unknown", Some("DE")), "unknown");
    }

    #[test]
    fn saving_the_displayed_form_back_never_changes_the_number() {
        // (stored, the customer's region, the practice's region)
        let cases = [
            ("+493012345678", "DE", "DE"),
            ("+48512345678", "DE", "DE"),
            ("+493012345678", "PL", "DE"),
            ("+48512345678", "PL", "DE"),
            ("+436641234567", "AT", "AT"),
        ];
        for (stored, customer, practice) in cases {
            let national_in = (customer == practice).then_some(practice);
            let shown = display_phone(stored, national_in);
            let saved = validate_phone("phone", &shown, customer).expect("the display form parses");
            assert_eq!(
                saved.e164, stored,
                "{stored} shown as {shown:?} for a customer in {customer}"
            );
        }
    }
}
