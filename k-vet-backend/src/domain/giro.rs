//! GiroCode — the QR code a German banking app scans to prefill a transfer.
//!
//! The payload follows the European Payments Council's "Quick Response Code Guidelines to Enable
//! Data Capture for the Initiation of a SEPA Credit Transfer" (EPC069-12), version **002**. That
//! version makes the BIC optional inside the SEPA area, which matters here: the practice may not
//! have entered one.
//!
//! Only the payload is built here, and it is a pure function so the exact bytes are testable.
//! Turning it into a picture is the PDF module's job.

use rust_decimal::Decimal;

/// Service tag, version, UTF-8, SEPA credit transfer.
const HEADER: [&str; 4] = ["BCD", "002", "1", "SCT"];

/// EPC069-12 caps the whole payload; a longer one produces a QR that readers reject.
const MAX_PAYLOAD_BYTES: usize = 331;
/// Field lengths from the specification.
const MAX_NAME: usize = 70;
const MAX_REMITTANCE: usize = 140;

/// The only currency the scheme carries.
const CURRENCY: &str = "EUR";
/// EPC069-12: `0.01` to `999999999.99`.
const MIN_AMOUNT: &str = "0.01";
const MAX_AMOUNT: &str = "999999999.99";

/// Builds the EPC069-12 payload, or `None` when this invoice cannot carry a GiroCode.
///
/// Returning `None` rather than an error is deliberate: a missing IBAN or a non-euro currency is
/// a perfectly ordinary configuration, and it should cost the practice a QR code on the invoice,
/// not the invoice itself.
pub fn epc_payload(
    beneficiary: &str,
    iban: &str,
    bic: &str,
    amount: Decimal,
    remittance: &str,
    currency: &str,
) -> Option<String> {
    if !currency.eq_ignore_ascii_case(CURRENCY) {
        return None;
    }

    // Banking apps want the compact form; humans type IBANs in groups of four.
    let iban: String = iban
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    if iban.is_empty() {
        return None;
    }

    let minimum = Decimal::from_str_exact(MIN_AMOUNT).ok()?;
    let maximum = Decimal::from_str_exact(MAX_AMOUNT).ok()?;
    if amount < minimum || amount > maximum {
        return None;
    }

    let bic: String = bic
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_uppercase();

    let lines = [
        HEADER[0],
        HEADER[1],
        HEADER[2],
        HEADER[3],
        &bic,
        &truncate(beneficiary.trim(), MAX_NAME),
        &iban,
        &format!("{CURRENCY}{:.2}", amount.round_dp(2)),
        // Purpose code and structured reference stay empty: this is a plain invoice, and the
        // unstructured remittance below is what a customer's statement should show.
        "",
        "",
        &truncate(remittance.trim(), MAX_REMITTANCE),
    ];
    let payload = lines.join("\n");

    // The trailing "beneficiary to originator" line is omitted, which the specification allows.
    (payload.len() <= MAX_PAYLOAD_BYTES).then_some(payload)
}

/// Cuts a string to at most `limit` bytes without splitting a character.
///
/// Umlauts matter here — the practice name is German, and slicing one in half would produce
/// invalid UTF-8 in the payload.
fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("test literal is a decimal")
    }

    // ⚠ FOR VET REVIEW (SC-005), item OFF-01 in `review.md`: scan the QR of one real invoice with
    // a banking app once and confirm it prefills the practice, the IBAN, the amount and the
    // invoice number. The account here is a placeholder; the amount is the total of RE-289.
    #[test]
    fn a_complete_invoice_yields_the_epc_payload() {
        let payload = epc_payload(
            "Mobile Tierärztin Dr. Erika Musterfrau",
            "DE02 1203 0000 0000 2020 51",
            "BYLADEM1001",
            dec("215.70"),
            "Rechnung RE-289",
            "EUR",
        )
        .expect("a complete invoice can carry a GiroCode");

        assert_eq!(
            payload,
            concat!(
                "BCD\n",
                "002\n",
                "1\n",
                "SCT\n",
                "BYLADEM1001\n",
                "Mobile Tierärztin Dr. Erika Musterfrau\n",
                "DE02120300000000202051\n",
                "EUR215.70\n",
                "\n",
                "\n",
                "Rechnung RE-289",
            ),
        );
    }

    #[test]
    fn the_iban_loses_its_spaces_and_the_amount_uses_a_decimal_point() {
        let payload = epc_payload("Praxis", "de02 1203 0000", "", dec("1234.5"), "R-1", "EUR")
            .expect("payload");
        let lines: Vec<&str> = payload.split('\n').collect();
        assert_eq!(lines[6], "DE0212030000", "compact and upper case");
        assert_eq!(lines[7], "EUR1234.50", "always two decimals, never a comma");
    }

    #[test]
    fn the_bic_may_be_missing_because_version_002_allows_it() {
        let payload = epc_payload(
            "Praxis",
            "DE02120300000000202051",
            "",
            dec("10"),
            "R",
            "EUR",
        )
        .expect("a SEPA transfer needs no BIC");
        assert_eq!(payload.split('\n').nth(4), Some(""));
    }

    #[test]
    fn a_missing_iban_yields_no_code() {
        assert!(epc_payload("Praxis", "   ", "", dec("10"), "R", "EUR").is_none());
    }

    #[test]
    fn only_euro_amounts_are_carried() {
        assert!(epc_payload("Praxis", "DE02", "", dec("10"), "R", "CHF").is_none());
        assert!(epc_payload("Praxis", "DE02", "", dec("10"), "R", "eur").is_some());
    }

    #[test]
    fn amounts_outside_the_schemes_range_yield_no_code() {
        assert!(epc_payload("Praxis", "DE02", "", Decimal::ZERO, "R", "EUR").is_none());
        assert!(epc_payload("Praxis", "DE02", "", dec("-5"), "R", "EUR").is_none());
        assert!(epc_payload("Praxis", "DE02", "", dec("0.01"), "R", "EUR").is_some());
        assert!(epc_payload("Praxis", "DE02", "", dec("1000000000"), "R", "EUR").is_none());
    }

    #[test]
    fn long_names_are_cut_on_a_character_boundary() {
        // 35 umlauts are 70 bytes — exactly the limit — so a 36th must be dropped whole.
        let name = "ä".repeat(36);
        let payload =
            epc_payload(&name, "DE02120300000000202051", "", dec("10"), "R", "EUR").expect("valid");
        let carried = payload.split('\n').nth(5).expect("name line");
        assert_eq!(carried.chars().count(), 35);
        assert!(carried.chars().all(|character| character == 'ä'));
    }

    #[test]
    fn an_oversized_payload_yields_no_code() {
        // A 140-character remittance plus a 70-character name still fits; the guard exists for
        // pathological input, so prove it triggers rather than silently emitting a broken QR.
        let long_iban = "D".repeat(300);
        assert!(epc_payload("Praxis", &long_iban, "", dec("10"), "R", "EUR").is_none());
    }
}
