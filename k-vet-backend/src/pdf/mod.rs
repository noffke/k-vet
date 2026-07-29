//! Invoice PDF rendering with an embedded Typst template (T034).
//!
//! Invoices are always German (FR-030), so every value is formatted de-DE **here** and the
//! template only places strings. That keeps locale handling in one place and makes the
//! formatting unit-testable without compiling a PDF.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use typst::foundations::{Bytes, Dict, IntoValue, Str};
use typst_as_lib::TypstEngine;
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_layout::PagedDocument;

use crate::config::Config;
use crate::domain::enums::Salutation;
use crate::domain::money::VatGroup;
use crate::error::{AppError, AppResult};

/// The template compiled into the binary; the config may point at a file instead.
const DEFAULT_TEMPLATE: &str = include_str!("../../templates/invoice.typ");

/// Everything the template needs, pre-formatted in German conventions.
#[derive(Debug, Serialize)]
pub struct InvoiceDocument {
    pub practice: PracticeBlock,
    pub invoice: InvoiceBlock,
}

#[derive(Debug, Serialize)]
pub struct PracticeBlock {
    pub name: String,
    pub address: String,
    pub iban: String,
    pub ustid: String,
    pub logo_present: bool,
}

#[derive(Debug, Serialize)]
pub struct InvoiceBlock {
    pub number: String,
    pub date: String,
    pub treatment_date: String,
    /// Address block lines: recipient name(s) followed by the address.
    pub recipient: Vec<String>,
    pub patients: Vec<String>,
    pub treatment_reason: String,
    /// Only filled when the invoice was created with "includes finding".
    pub finding: String,
    pub items: Vec<InvoiceLine>,
    pub vat_groups: Vec<InvoiceVatGroup>,
    pub total: String,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct InvoiceLine {
    pub position: i32,
    pub name: String,
    pub patient: String,
    pub quantity: String,
    pub unit: String,
    pub factor: String,
    pub got_number: String,
    pub km: String,
    pub price: String,
    pub total: String,
}

#[derive(Debug, Serialize)]
pub struct InvoiceVatGroup {
    pub rate: String,
    pub net: String,
    pub vat: String,
}

/// Renders the invoice to PDF bytes.
pub fn render_invoice(
    config: &Config,
    document: &InvoiceDocument,
    logo: Option<Vec<u8>>,
) -> AppResult<Vec<u8>> {
    let template = load_template(config)?;
    let data = serde_json::to_string(document)
        .map_err(|error| AppError::internal("serializing invoice data", error))?;

    let engine = TypstEngine::builder()
        .main_file(template)
        // Only the fonts bundled with the binary — the Pi has no font packages installed.
        .search_fonts_with(
            TypstKitFontOptions::new()
                .include_system_fonts(false)
                .include_embedded_fonts(true),
        )
        .build();

    let mut inputs = Dict::new();
    inputs.insert(Str::from("data"), data.into_value());
    inputs.insert(
        Str::from("logo"),
        Bytes::new(logo.unwrap_or_default()).into_value(),
    );

    let compiled = engine.compile_with_input::<_, PagedDocument>(inputs);
    for warning in &compiled.warnings {
        tracing::warn!(message = %warning.message, "typst warning while rendering the invoice");
    }
    let paged = compiled
        .output
        .map_err(|error| AppError::internal("compiling the invoice template", error))?;

    typst_pdf::pdf(&paged, &typst_pdf::PdfOptions::default())
        .map_err(|errors| AppError::internal("writing the invoice PDF", format!("{errors:?}")))
}

fn load_template(config: &Config) -> AppResult<String> {
    match &config.invoice.typst_template {
        Some(path) => std::fs::read_to_string(path).map_err(|error| {
            AppError::internal(
                format!("reading the invoice template {}", path.display()),
                error,
            )
        }),
        None => Ok(DEFAULT_TEMPLATE.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// German formatting helpers — the single place where money and dates become text.
// ---------------------------------------------------------------------------

/// `1234.5` → `"1.234,50 €"`.
pub fn money_de(value: Decimal, currency: &str) -> String {
    let symbol = match currency {
        "EUR" => "€",
        other => other,
    };
    format!("{} {symbol}", group_de(value, 2))
}

/// Quantities and factors: German decimal comma, trailing zeros trimmed.
pub fn number_de(value: Decimal) -> String {
    let normalized = value.normalize();
    let decimals = normalized.scale().min(3);
    group_de(normalized, decimals)
}

/// Percentages as `"19 %"`.
pub fn percent_de(value: Decimal) -> String {
    format!("{} %", number_de(value))
}

/// `2026-05-04` → `"04.05.2026"`.
pub fn date_de(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

/// Thousands separated by `.`, decimals by `,`.
fn group_de(value: Decimal, decimals: u32) -> String {
    let rounded = value.round_dp(decimals);
    let text = format!("{:.*}", decimals as usize, rounded);
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (integer, fraction) = match digits.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (digits, None),
    };

    let mut grouped = String::new();
    for (index, character) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(character);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped},{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}

/// The customer columns the invoice address block is built from.
#[derive(Debug, Default, Clone)]
pub struct CustomerAddress {
    pub salutation: Option<Salutation>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub second_salutation: Option<Salutation>,
    pub second_first_name: Option<String>,
    pub second_last_name: Option<String>,
    /// Database-generated: both second-name parts are present.
    pub has_second_name: bool,
    pub home_addon: Option<String>,
    pub home_street: Option<String>,
    pub home_zip: Option<String>,
    pub home_city: Option<String>,
    pub invoice_salutation: Option<Salutation>,
    pub invoice_first_name: Option<String>,
    pub invoice_last_name: Option<String>,
    pub invoice_addon: Option<String>,
    pub invoice_street: Option<String>,
    pub invoice_zip: Option<String>,
    pub invoice_city: Option<String>,
    /// Database-generated: the invoice address group is complete.
    pub has_invoice_address: bool,
}

/// Builds the invoice address block (FR-030).
///
/// With an invoice address set, the PDF shows only that recipient at that address.
/// Without one it shows the customer name, the second name as its own line when present,
/// and the home address.
pub fn address_block(customer: &CustomerAddress) -> Vec<String> {
    let name_line =
        |salutation: Option<Salutation>, first: &Option<String>, last: &Option<String>| {
            [
                salutation.map(|value| value.to_string()),
                first.clone().filter(|value| !value.trim().is_empty()),
                last.clone().filter(|value| !value.trim().is_empty()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
        };

    let mut lines = Vec::new();
    if customer.has_invoice_address {
        lines.push(name_line(
            customer.invoice_salutation,
            &customer.invoice_first_name,
            &customer.invoice_last_name,
        ));
        lines.extend(customer.invoice_addon.clone());
        lines.extend(customer.invoice_street.clone());
        lines.push(zip_city(&customer.invoice_zip, &customer.invoice_city));
    } else {
        lines.push(name_line(
            customer.salutation,
            &customer.first_name,
            &customer.last_name,
        ));
        if customer.has_second_name {
            lines.push(name_line(
                customer.second_salutation,
                &customer.second_first_name,
                &customer.second_last_name,
            ));
        }
        lines.extend(customer.home_addon.clone());
        lines.extend(customer.home_street.clone());
        lines.push(zip_city(&customer.home_zip, &customer.home_city));
    }
    lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn zip_city(zip: &Option<String>, city: &Option<String>) -> String {
    [zip.clone(), city.clone()]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Formats the VAT summary for the template.
pub fn vat_groups_de(groups: &[VatGroup], currency: &str) -> Vec<InvoiceVatGroup> {
    groups
        .iter()
        .map(|group| InvoiceVatGroup {
            rate: percent_de(group.vat_percent),
            net: money_de(group.net, currency),
            vat: money_de(group.vat, currency),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("test literal is a decimal")
    }

    #[test]
    fn money_uses_german_conventions() {
        assert_eq!(money_de(dec("1234.5"), "EUR"), "1.234,50 €");
        assert_eq!(money_de(dec("0.5"), "EUR"), "0,50 €");
        assert_eq!(money_de(dec("1234567.89"), "EUR"), "1.234.567,89 €");
        assert_eq!(money_de(dec("-12.30"), "EUR"), "-12,30 €");
        assert_eq!(money_de(dec("42"), "CHF"), "42,00 CHF");
    }

    #[test]
    fn numbers_trim_trailing_zeros() {
        assert_eq!(number_de(dec("1.50")), "1,5");
        assert_eq!(number_de(dec("100.00")), "100");
        assert_eq!(number_de(dec("0.125")), "0,125");
    }

    fn customer() -> CustomerAddress {
        CustomerAddress {
            salutation: Some(Salutation::Frau),
            first_name: Some("Erika".to_owned()),
            last_name: Some("Mustermann".to_owned()),
            home_street: Some("Musterweg 5".to_owned()),
            home_zip: Some("12345".to_owned()),
            home_city: Some("Musterstadt".to_owned()),
            ..CustomerAddress::default()
        }
    }

    #[test]
    fn address_block_uses_the_home_address_by_default() {
        assert_eq!(
            address_block(&customer()),
            vec!["Frau Erika Mustermann", "Musterweg 5", "12345 Musterstadt"]
        );
    }

    #[test]
    fn address_block_adds_the_second_name_on_its_own_line() {
        let mut with_second = customer();
        with_second.has_second_name = true;
        with_second.second_salutation = Some(Salutation::Herr);
        with_second.second_first_name = Some("Max".to_owned());
        with_second.second_last_name = Some("Mustermann".to_owned());

        assert_eq!(
            address_block(&with_second),
            vec![
                "Frau Erika Mustermann",
                "Herr Max Mustermann",
                "Musterweg 5",
                "12345 Musterstadt",
            ]
        );
    }

    #[test]
    fn an_invoice_address_replaces_the_customer_block_entirely() {
        let mut with_invoice = customer();
        with_invoice.has_second_name = true;
        with_invoice.second_salutation = Some(Salutation::Herr);
        with_invoice.second_last_name = Some("Mustermann".to_owned());
        with_invoice.has_invoice_address = true;
        with_invoice.invoice_salutation = Some(Salutation::Familie);
        with_invoice.invoice_last_name = Some("Mustermann".to_owned());
        with_invoice.invoice_addon = Some("c/o Fr. Müller".to_owned());
        with_invoice.invoice_street = Some("Rechnungsallee 9".to_owned());
        with_invoice.invoice_zip = Some("54321".to_owned());
        with_invoice.invoice_city = Some("Zahlstadt".to_owned());

        assert_eq!(
            address_block(&with_invoice),
            vec![
                "Familie Mustermann",
                "c/o Fr. Müller",
                "Rechnungsallee 9",
                "54321 Zahlstadt",
            ],
            "neither the customer name nor the home address appear"
        );
    }

    #[test]
    fn address_block_skips_empty_lines() {
        let sparse = CustomerAddress {
            last_name: Some("Mustermann".to_owned()),
            ..CustomerAddress::default()
        };
        assert_eq!(address_block(&sparse), vec!["Mustermann"]);
    }

    #[test]
    fn percentages_and_dates_are_german() {
        assert_eq!(percent_de(dec("19.000")), "19 %");
        assert_eq!(percent_de(dec("7.5")), "7,5 %");
        let date = NaiveDate::from_ymd_opt(2026, 5, 4).expect("valid date");
        assert_eq!(date_de(date), "04.05.2026");
    }

    /// Compiles the embedded template with realistic data — the guard against a template
    /// change that only breaks at invoicing time.
    #[test]
    fn renders_the_embedded_template_to_a_pdf() {
        let config = crate::config::Config::parse(
            r#"
            [server]
            [auth]
            username = "tierarzt"
            password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
            [database]
            url = "postgres://unused/unused"
            [storage]
            attachments_dir = "/tmp"
            [mail]
            smtp_host = "localhost"
            from_address = "praxis@example.com"
            [invoice]
            number_pattern = "{year}-{counter:4}"
            [travel_expenses]
            rate_per_double_km = "3.50"
            minimum = "13.00"
            "#,
            "test.toml",
        )
        .expect("test config");

        let document = InvoiceDocument {
            practice: PracticeBlock {
                name: "Tierärztin Dr. Käthe Musterfrau".to_owned(),
                address: "Musterweg 5\n12345 Musterstadt".to_owned(),
                iban: "DE02120300000000202051".to_owned(),
                ustid: "DE123456789".to_owned(),
                logo_present: false,
            },
            invoice: InvoiceBlock {
                number: "2026-0042".to_owned(),
                date: "04.05.2026".to_owned(),
                treatment_date: "03.05.2026".to_owned(),
                recipient: vec![
                    "Frau Erika Mustermann".to_owned(),
                    "Musterstraße 1".to_owned(),
                    "12345 Musterstadt".to_owned(),
                ],
                patients: vec!["Bello".to_owned()],
                treatment_reason: "Routinekontrolle".to_owned(),
                finding: "Ohne Befund".to_owned(),
                items: vec![
                    InvoiceLine {
                        position: 1,
                        name: "Allgemeine Untersuchung".to_owned(),
                        patient: "Bello".to_owned(),
                        quantity: "1".to_owned(),
                        unit: String::new(),
                        factor: "100 %".to_owned(),
                        got_number: "1".to_owned(),
                        km: String::new(),
                        price: "23,62 €".to_owned(),
                        total: "23,62 €".to_owned(),
                    },
                    InvoiceLine {
                        position: 2,
                        name: "Amoxicillin 100".to_owned(),
                        patient: "Bello".to_owned(),
                        quantity: "30".to_owned(),
                        unit: "ml".to_owned(),
                        factor: String::new(),
                        got_number: String::new(),
                        km: String::new(),
                        price: "0,50 €".to_owned(),
                        total: "15,00 €".to_owned(),
                    },
                ],
                vat_groups: vec![InvoiceVatGroup {
                    rate: "19 %".to_owned(),
                    net: "32,45 €".to_owned(),
                    vat: "6,17 €".to_owned(),
                }],
                total: "38,62 €".to_owned(),
                note: "Vielen Dank für Ihr Vertrauen.".to_owned(),
            },
        };

        let pdf = render_invoice(&config, &document, None).expect("the template compiles");
        assert!(pdf.starts_with(b"%PDF-"), "output is a PDF");
        assert!(
            pdf.len() > 2_000,
            "a one-page invoice is a few kilobytes, got {}",
            pdf.len()
        );
    }
}
