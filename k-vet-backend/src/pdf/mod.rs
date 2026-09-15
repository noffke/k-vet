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

/// Source Sans 3 (SIL Open Font License 1.1, `fonts/LICENSE.txt`) — regular, bold, italic and
/// bold italic, the four the default template asks for.
const SANS_FACES: [&[u8]; 4] = [
    include_bytes!("../../fonts/SourceSans3-Regular.otf"),
    include_bytes!("../../fonts/SourceSans3-Bold.otf"),
    include_bytes!("../../fonts/SourceSans3-Italic.otf"),
    include_bytes!("../../fonts/SourceSans3-BoldItalic.otf"),
];

/// Everything the template needs, pre-formatted in German conventions.
#[derive(Debug, Serialize)]
pub struct InvoiceDocument {
    pub practice: PracticeBlock,
    pub invoice: InvoiceBlock,
}

#[derive(Debug, Serialize)]
pub struct PracticeBlock {
    pub name: String,
    /// Street and town on separate lines, for the letterhead.
    pub address: String,
    /// The same address comma-joined, for the one-line footer and the sender line.
    pub address_line: String,
    pub email: String,
    pub iban: String,
    pub bic: String,
    pub bank_name: String,
    pub ustid: String,
    pub logo_present: bool,
}

#[derive(Debug, Serialize)]
pub struct InvoiceBlock {
    pub number: String,
    pub date: String,
    /// Printed as its own labelled line so document recognition can pick it up.
    pub due_date: String,
    pub treatment_date: String,
    /// Address block lines: recipient name(s) followed by the address.
    pub recipient: Vec<String>,
    /// The practice on one line, above the recipient block (DIN 5008 Rücksendeangabe).
    pub sender_line: String,
    /// Ready-made salutation, e.g. `Sehr geehrter Herr Mustermann`.
    pub greeting: String,
    pub patients: Vec<String>,
    /// e.g. `Behandlung/Konsultation Eddie (Hund – Havaneser) am 28.07.2026`.
    pub treatment_heading: String,
    /// The clinical report, one block per animal — the reason and, when the invoice was
    /// created with "includes finding", the finding. Animals with neither are left out.
    pub reports: Vec<PatientReport>,
    /// Lines grouped per animal, the way the invoice prints them.
    pub patient_groups: Vec<PatientGroup>,
    /// Every line in one flat list, kept so templates written before grouping still work.
    pub items: Vec<InvoiceLine>,
    pub vat_groups: Vec<InvoiceVatGroup>,
    pub total: String,
    pub note: String,
    pub qr_present: bool,
    /// The GiroCode payload, kept out of the template data: the template places the picture,
    /// it has no use for the bytes behind it.
    #[serde(skip)]
    pub qr_payload: Option<String>,
}

/// What was treated on one animal, and what was found.
#[derive(Debug, Serialize)]
pub struct PatientReport {
    pub patient: String,
    /// e.g. `Hund, Havaneser, Geburtsdatum: 01.01.2021`.
    pub description: String,
    pub treatment_reason: String,
    /// Empty unless the invoice was created with "Befund aufführen".
    pub finding: String,
}

/// The billing lines of one animal, under a heading that identifies it.
#[derive(Debug, Serialize)]
pub struct PatientGroup {
    /// Empty for lines the vet did not attribute to an animal; the template then omits the
    /// `Tier:` heading and simply lists them.
    pub patient: String,
    /// e.g. `Hund, Havaneser, Geburtsdatum: 01.01.2021`.
    pub description: String,
    pub service_date: String,
    pub items: Vec<InvoiceLine>,
}

#[derive(Debug, Serialize, Clone)]
pub struct InvoiceLine {
    pub position: i32,
    pub name: String,
    pub patient: String,
    pub quantity: String,
    pub unit: String,
    pub factor: String,
    pub got_number: String,
    pub km: String,
    /// VAT rate of this line, e.g. `19 %`.
    pub vat: String,
    /// The small grey second line: `GOT-Nr. 16`, `GOT-Nr. 16 (§8)`, `1 Stück`, or
    /// `1 Originalpackung, Zulassungsnr: 402485.00.00`.
    pub detail: String,
    /// Gross unit price and gross line total — what a private customer expects to read.
    pub price: String,
    pub total: String,
}

#[derive(Debug, Serialize)]
pub struct InvoiceVatGroup {
    pub rate: String,
    pub net: String,
    pub vat: String,
    pub gross: String,
}

/// Renders the invoice to PDF bytes.
pub fn render_invoice(
    config: &Config,
    document: &InvoiceDocument,
    logo: Option<Vec<u8>>,
    qr: Option<String>,
) -> AppResult<Vec<u8>> {
    let template = load_template(config)?;
    let data = serde_json::to_string(document)
        .map_err(|error| AppError::internal("serializing invoice data", error))?;

    let engine = TypstEngine::builder()
        .main_file(template)
        // The invoice sets in Source Sans 3, which typst-assets does not carry: its embedded
        // text faces are all serif (Libertinus Serif, New Computer Modern) and its only sans is
        // DejaVu Sans *Mono*. So the four faces the template needs travel in the binary.
        .fonts(SANS_FACES)
        // Everything else stays bundled too — the Pi has no font packages installed, and a
        // custom template may still want the serif faces.
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
    // An SVG rather than a bitmap: a QR is pure rectangles, so it stays sharp at any print size
    // and needs none of the fonts the binary does not carry.
    inputs.insert(
        Str::from("qr"),
        Bytes::new(qr.unwrap_or_default().into_bytes()).into_value(),
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
    /// Optional company; printed above the name, never instead of it.
    pub company: Option<String>,
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
    pub invoice_company: Option<String>,
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
///
/// A company, where there is one, goes **above** the name — DIN 5008 order, and the reason it is
/// its own column rather than something typed into `*_addon`, which prints below.
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
        lines.extend(customer.invoice_company.clone());
        lines.push(name_line(
            customer.invoice_salutation,
            &customer.invoice_first_name,
            &customer.invoice_last_name,
        ));
        lines.extend(customer.invoice_addon.clone());
        lines.extend(customer.invoice_street.clone());
        lines.push(zip_city(&customer.invoice_zip, &customer.invoice_city));
    } else {
        lines.extend(customer.company.clone());
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
            gross: money_de(group.gross, currency),
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
    fn a_company_is_the_first_line_of_the_home_address() {
        let mut business = customer();
        business.company = Some("Hundepension Musterhof GmbH".to_owned());

        assert_eq!(
            address_block(&business),
            vec![
                "Hundepension Musterhof GmbH",
                "Frau Erika Mustermann",
                "Musterweg 5",
                "12345 Musterstadt",
            ],
            "the company goes above the name, not below it like an addon",
        );
    }

    #[test]
    fn an_invoice_address_carries_its_own_company() {
        let mut business = customer();
        business.company = Some("Hundepension Musterhof GmbH".to_owned());
        business.has_invoice_address = true;
        business.invoice_company = Some("Musterhof Verwaltungs KG".to_owned());
        business.invoice_salutation = Some(Salutation::Herr);
        business.invoice_last_name = Some("Buchhalter".to_owned());
        business.invoice_street = Some("Rechnungsallee 9".to_owned());
        business.invoice_zip = Some("54321".to_owned());
        business.invoice_city = Some("Zahlstadt".to_owned());

        assert_eq!(
            address_block(&business),
            vec![
                "Musterhof Verwaltungs KG",
                "Herr Buchhalter",
                "Rechnungsallee 9",
                "54321 Zahlstadt",
            ],
            "the home company must not leak into the invoice recipient",
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
            // A company the vet typed and cleared again must not leave a blank line behind.
            company: Some("   ".to_owned()),
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
    fn empty_config() -> crate::config::Config {
        crate::config::Config::parse(
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
        .expect("test config")
    }

    /// Compiles the embedded template with realistic data — the guard against a template
    /// change that only breaks at invoicing time.
    #[test]
    fn renders_the_embedded_template_to_a_pdf() {
        let config = empty_config();

        let examination = InvoiceLine {
            position: 1,
            name: "Allgemeine Untersuchung".to_owned(),
            patient: "Bello".to_owned(),
            quantity: "1".to_owned(),
            unit: String::new(),
            factor: "100 %".to_owned(),
            got_number: "1".to_owned(),
            km: String::new(),
            vat: "19 %".to_owned(),
            detail: "GOT-Nr. 1".to_owned(),
            price: "28,11 €".to_owned(),
            total: "28,11 €".to_owned(),
        };
        let medicine = InvoiceLine {
            position: 2,
            name: "Amoxicillin 100".to_owned(),
            patient: "Minka".to_owned(),
            quantity: "30".to_owned(),
            unit: "ml".to_owned(),
            factor: String::new(),
            got_number: String::new(),
            km: String::new(),
            vat: "7 %".to_owned(),
            detail: "1 Originalpackung, Zulassungsnr: 402485.00.00".to_owned(),
            price: "0,50 €".to_owned(),
            total: "15,00 €".to_owned(),
        };

        let document = InvoiceDocument {
            practice: PracticeBlock {
                name: "Tierärztin Dr. Käthe Musterfrau".to_owned(),
                address: "Musterweg 5\n12345 Musterstadt".to_owned(),
                address_line: "Musterweg 5, 12345 Musterstadt".to_owned(),
                email: "praxis@example.com".to_owned(),
                iban: "DE02120300000000202051".to_owned(),
                bic: "BYLADEM1001".to_owned(),
                bank_name: "Musterbank".to_owned(),
                ustid: "DE123456789".to_owned(),
                logo_present: false,
            },
            invoice: InvoiceBlock {
                number: "2026-0042".to_owned(),
                date: "04.05.2026".to_owned(),
                due_date: "18.05.2026".to_owned(),
                treatment_date: "03.05.2026".to_owned(),
                recipient: vec![
                    "Frau Erika Mustermann".to_owned(),
                    "Musterstraße 1".to_owned(),
                    "12345 Musterstadt".to_owned(),
                ],
                sender_line: "Tierärztin Dr. Käthe Musterfrau, Musterweg 5, 12345 Musterstadt"
                    .to_owned(),
                greeting: "Sehr geehrte Frau Mustermann".to_owned(),
                patients: vec!["Bello".to_owned(), "Minka".to_owned()],
                treatment_heading: "Behandlung/Konsultation Bello (Hund – Havaneser), \
                                    Minka (Katze) am 03.05.2026"
                    .to_owned(),
                reports: vec![
                    PatientReport {
                        patient: "Bello".to_owned(),
                        description: "Hund, Havaneser, Geburtsdatum: 01.01.2021".to_owned(),
                        treatment_reason: "Routinekontrolle".to_owned(),
                        finding: "Ohne Befund".to_owned(),
                    },
                    PatientReport {
                        patient: "Minka".to_owned(),
                        description: "Katze".to_owned(),
                        treatment_reason: "Impfung".to_owned(),
                        finding: String::new(),
                    },
                ],
                patient_groups: vec![
                    PatientGroup {
                        patient: "Bello".to_owned(),
                        description: "Hund, Havaneser, Geburtsdatum: 01.01.2021".to_owned(),
                        service_date: "03.05.2026".to_owned(),
                        items: vec![examination.clone()],
                    },
                    PatientGroup {
                        patient: "Minka".to_owned(),
                        description: "Katze".to_owned(),
                        service_date: "03.05.2026".to_owned(),
                        items: vec![medicine.clone()],
                    },
                ],
                items: vec![examination, medicine],
                vat_groups: vec![
                    InvoiceVatGroup {
                        rate: "19 %".to_owned(),
                        net: "23,62 €".to_owned(),
                        vat: "4,49 €".to_owned(),
                        gross: "28,11 €".to_owned(),
                    },
                    InvoiceVatGroup {
                        rate: "7 %".to_owned(),
                        net: "14,02 €".to_owned(),
                        vat: "0,98 €".to_owned(),
                        gross: "15,00 €".to_owned(),
                    },
                ],
                total: "43,11 €".to_owned(),
                note: "Vielen Dank für Ihr Vertrauen.".to_owned(),
                qr_present: true,
                qr_payload: None,
            },
        };

        let qr = crate::domain::giro::epc_payload(
            "Tierärztin Dr. Käthe Musterfrau",
            "DE02120300000000202051",
            "BYLADEM1001",
            Decimal::from_str_exact("43.11").expect("amount"),
            "Rechnung 2026-0042",
            "EUR",
        )
        .and_then(|payload| {
            use fast_qr::convert::{Builder, Shape, svg::SvgBuilder};
            let code = fast_qr::QRBuilder::new(payload)
                .ecl(fast_qr::ECL::M)
                .build()
                .ok()?;
            Some(SvgBuilder::default().shape(Shape::Square).to_str(&code))
        });
        assert!(qr.is_some(), "the sample invoice can carry a GiroCode");

        let pdf = render_invoice(&config, &document, None, qr).expect("the template compiles");
        assert!(pdf.starts_with(b"%PDF-"), "output is a PDF");
        assert!(
            pdf.len() > 2_000,
            "a one-page invoice is a few kilobytes, got {}",
            pdf.len()
        );
        // The sans faces are the one part of the template that is not in typst-assets: an
        // unknown family falls back to the bundled serif with only a warning, which is a
        // regression nobody notices until an invoice is printed. The subset PostScript name is
        // written into the PDF, so look for it (typst prefixes it with a six-letter subset tag).
        assert!(
            pdf.windows(12).any(|window| window == b"SourceSans3-"),
            "the invoice must set in Source Sans 3, not fall back to the bundled serif"
        );
    }

    /// A fresh install has no logo, no bank details and no findings. The template must still
    /// produce a usable invoice rather than a page of stray separators — or an error.
    #[test]
    fn renders_with_everything_the_practice_has_not_filled_in_yet() {
        let config = empty_config();
        let document = minimal_document();

        let pdf = render_invoice(&config, &document, None, None).expect("the template compiles");
        assert!(pdf.starts_with(b"%PDF-"), "output is a PDF");
    }

    /// The least an invoice can be: one line, one VAT group, nothing optional filled in.
    fn minimal_document() -> InvoiceDocument {
        InvoiceDocument {
            practice: PracticeBlock {
                name: "Praxis".to_owned(),
                address: String::new(),
                address_line: String::new(),
                email: String::new(),
                iban: String::new(),
                bic: String::new(),
                bank_name: String::new(),
                ustid: String::new(),
                logo_present: false,
            },
            invoice: InvoiceBlock {
                number: "2026-0001".to_owned(),
                date: "04.05.2026".to_owned(),
                due_date: String::new(),
                treatment_date: String::new(),
                recipient: vec!["Herr Mustermann".to_owned()],
                sender_line: "Praxis".to_owned(),
                greeting: "Sehr geehrter Herr Mustermann".to_owned(),
                patients: Vec::new(),
                treatment_heading: String::new(),
                reports: Vec::new(),
                patient_groups: vec![PatientGroup {
                    patient: String::new(),
                    description: String::new(),
                    service_date: String::new(),
                    items: vec![InvoiceLine {
                        position: 1,
                        name: "Beratung".to_owned(),
                        patient: String::new(),
                        quantity: "1".to_owned(),
                        unit: String::new(),
                        factor: String::new(),
                        got_number: String::new(),
                        km: String::new(),
                        vat: "19 %".to_owned(),
                        detail: String::new(),
                        price: "13,40 €".to_owned(),
                        total: "13,40 €".to_owned(),
                    }],
                }],
                items: Vec::new(),
                vat_groups: vec![InvoiceVatGroup {
                    rate: "19 %".to_owned(),
                    net: "11,26 €".to_owned(),
                    vat: "2,14 €".to_owned(),
                    gross: "13,40 €".to_owned(),
                }],
                total: "13,40 €".to_owned(),
                note: String::new(),
                qr_present: false,
                qr_payload: None,
            },
        }
    }

    /// A practice logo, which until now no render test passed at all — which is why it shipped
    /// printing off the top of the page (issues.md 9).
    ///
    /// The geometry itself is checked end to end, where the page can be rasterised and looked
    /// at; what is worth pinning here is that a logo renders at all and reaches the output.
    /// `KVET_PDF_DUMP=/tmp/logo.pdf` writes the result out to be inspected by eye.
    #[test]
    fn renders_a_letterhead_logo() {
        let config = empty_config();
        let bare = render_invoice(&config, &minimal_document(), None, None)
            .expect("renders without a logo");

        let mut document = minimal_document();
        document.practice.logo_present = true;
        let with_logo = render_invoice(&config, &document, Some(BANDED_LOGO.to_vec()), None)
            .expect("the template compiles with a logo");

        assert!(with_logo.starts_with(b"%PDF-"), "output is a PDF");
        assert!(
            with_logo.len() > bare.len(),
            "the logo has to reach the document: {} bytes with it, {} without",
            with_logo.len(),
            bare.len(),
        );

        if let Ok(path) = std::env::var("KVET_PDF_DUMP") {
            std::fs::write(&path, &with_logo).expect("dump the pdf");
        }
    }

    /// Three colour bands, so a vertical crop is unmistakable in a dump. SVG because the
    /// settings page accepts one and it needs no image crate to build here; `rgb()` rather than
    /// hex because `"#` would close the raw string.
    const BANDED_LOGO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 200">
        <rect width="600" height="67" fill="rgb(220,60,40)"/>
        <rect y="67" width="600" height="66" fill="rgb(60,160,80)"/>
        <rect y="133" width="600" height="67" fill="rgb(40,80,200)"/>
    </svg>"#;
}
