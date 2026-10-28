//! The customers as contacts in the phone's address book.
//!
//! Each contact is written when it is asked for, as vCard 3.0 — the version every sync client
//! reads — from the customer, the e-mail addresses and the living animals; its ETag is a hash
//! of that text, as for the calendar. The name shown is "Erika Mustermann (Bello, Minka)": what
//! the incoming-call screen says, and what a search by an animal's name finds.
//!
//! The phone may create and edit contacts. A new one becomes a customer — a draft until the vet
//! completes it, since it carries no salutation — and an edit changes the name, the numbers,
//! the e-mail addresses and the address. The "(Bello, Minka)" k-vet adds is never read back into
//! the name, and a deletion is not taken: the customer stays, and the contact comes back at the
//! next sync. A customer's record is k-vet's, with animals and invoices behind it.

use std::collections::HashSet;

use calcard::vcard::{
    VCard, VCardEntry, VCardParameterName, VCardParameterValue, VCardProperty, VCardType,
    VCardValue,
};
use sqlx::PgConnection;

use crate::AppState;
use crate::dav::object::{Conditions, Object, WriteError, Written, etag_of};
use crate::domain::contact::{validate_email, validate_phone};
use crate::error::AppResult;

/// A customer with everything the contact shows.
#[derive(Debug, Clone, Default)]
pub struct ContactSource {
    pub id: i64,
    pub uid: Option<String>,
    pub href: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub second_name: Option<String>,
    pub phone: Option<String>,
    pub mobile: Option<String>,
    pub emails: Vec<String>,
    pub street: Option<String>,
    pub zip: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    /// The living animals' names, for the name shown.
    pub animals: Vec<String>,
    /// "Bello (Hund)" — name and species, never the breed — for the note.
    pub animals_with_species: Vec<String>,
}

impl ContactSource {
    pub fn name(&self) -> String {
        self.href
            .clone()
            .unwrap_or_else(|| format!("kvet-customer-{}.vcf", self.id))
    }
}

/// Escapes a vCard text value (RFC 6350 §3.4).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Folds a content line at 75 octets, never inside a character (RFC 6350 §3.2).
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for character in line.chars() {
        let length = character.len_utf8();
        if width + length > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(character);
        width += length;
    }
    out.push_str("\r\n");
}

/// The contact's vCard. `link` is where the customer opens in k-vet.
pub fn contact_vcf(source: &ContactSource, link: &str) -> String {
    let person = [source.first_name.as_deref(), source.last_name.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let named = if person.is_empty() {
        source.company.clone().unwrap_or_default()
    } else {
        person
    };
    let shown = if source.animals.is_empty() {
        named
    } else {
        format!("{named} ({})", source.animals.join(", "))
    };
    let mut lines = vec![
        "BEGIN:VCARD".to_owned(),
        "VERSION:3.0".to_owned(),
        "PRODID:-//k-vet//Kunden//DE".to_owned(),
        format!(
            "UID:{}",
            escape(
                &source
                    .uid
                    .clone()
                    .unwrap_or_else(|| format!("kvet-customer-{}", source.id))
            )
        ),
        format!("FN:{}", escape(&shown)),
        format!(
            "N:{};{};;;",
            escape(source.last_name.as_deref().unwrap_or_default()),
            escape(source.first_name.as_deref().unwrap_or_default())
        ),
    ];
    if let Some(company) = source.company.as_deref().filter(|text| !text.is_empty()) {
        lines.push(format!("ORG:{}", escape(company)));
    }
    if let Some(phone) = &source.phone {
        lines.push(format!("TEL;TYPE=VOICE:{}", escape(phone)));
    }
    if let Some(mobile) = &source.mobile {
        lines.push(format!("TEL;TYPE=CELL:{}", escape(mobile)));
    }
    for email in &source.emails {
        lines.push(format!("EMAIL;TYPE=INTERNET:{}", escape(email)));
    }
    if source.street.is_some() || source.city.is_some() || source.zip.is_some() {
        lines.push(format!(
            "ADR;TYPE=HOME:;;{};{};;{};{}",
            escape(source.street.as_deref().unwrap_or_default()),
            escape(source.city.as_deref().unwrap_or_default()),
            escape(source.zip.as_deref().unwrap_or_default()),
            escape(source.country.as_deref().unwrap_or_default())
        ));
    }
    let mut note = Vec::new();
    if let Some(second) = &source.second_name {
        note.push(format!("Auch: {second}"));
    }
    if !source.animals_with_species.is_empty() {
        note.push(source.animals_with_species.join(", "));
    }
    note.push(link.to_owned());
    lines.push(format!("NOTE:{}", escape(&note.join("\n"))));
    lines.push("END:VCARD".to_owned());

    let mut out = String::new();
    for line in &lines {
        fold(line, &mut out);
    }
    out
}

/// The address book's customers: not archived, with a name or a company — drafts included, so
/// a contact made on the phone stays there while the vet completes it.
pub async fn sources(
    connection: &mut PgConnection,
    name: Option<&str>,
) -> AppResult<Vec<ContactSource>> {
    let rows = sqlx::query!(
        r#"SELECT customer.id, customer.dav_uid, customer.dav_href, customer.first_name,
                  customer.last_name, customer.company,
                  NULLIF(concat_ws(' ', customer.second_first_name, customer.second_last_name), '')
                      AS second_name,
                  customer.phone, customer.mobile, customer.home_street, customer.home_zip,
                  customer.home_city, customer.home_country,
                  ARRAY(SELECT email FROM customer_email
                         WHERE customer_email.customer_id = customer.id ORDER BY id) AS "emails!",
                  ARRAY(SELECT patient.name FROM patient
                         WHERE patient.customer_id = customer.id AND NOT patient.archived
                           AND patient.date_of_death IS NULL AND patient.name IS NOT NULL
                         ORDER BY patient.name) AS "animals!",
                  ARRAY(SELECT patient.name || COALESCE(' (' || patient.species || ')', '')
                          FROM patient
                         WHERE patient.customer_id = customer.id AND NOT patient.archived
                           AND patient.date_of_death IS NULL AND patient.name IS NOT NULL
                         ORDER BY patient.name) AS "animals_with_species!"
           FROM customer
           WHERE NOT customer.archived
             AND COALESCE(customer.last_name, customer.first_name, customer.company) IS NOT NULL
             AND ($1::text IS NULL
                  OR COALESCE(customer.dav_href, 'kvet-customer-' || customer.id || '.vcf') = $1)
           ORDER BY customer.id"#,
        name,
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ContactSource {
            id: row.id,
            uid: row.dav_uid,
            href: row.dav_href,
            first_name: row.first_name,
            last_name: row.last_name,
            company: row.company,
            second_name: row.second_name,
            phone: row.phone,
            mobile: row.mobile,
            emails: row.emails,
            street: row.home_street,
            zip: row.home_zip,
            city: row.home_city,
            country: row.home_country,
            animals: row.animals,
            animals_with_species: row.animals_with_species,
        })
        .collect())
}

fn object_of(state: &AppState, source: &ContactSource) -> Object {
    let link = format!(
        "{}/customers/{}",
        state.config.server.base_url.trim_end_matches('/'),
        source.id
    );
    let body = contact_vcf(source, &link);
    Object {
        name: source.name(),
        etag: etag_of(&body),
        body,
        starts_at: None,
    }
}

pub async fn objects(state: &AppState, connection: &mut PgConnection) -> AppResult<Vec<Object>> {
    Ok(sources(connection, None)
        .await?
        .iter()
        .map(|source| object_of(state, source))
        .collect())
}

pub async fn object(
    state: &AppState,
    connection: &mut PgConnection,
    name: &str,
) -> AppResult<Option<Object>> {
    Ok(sources(connection, Some(name))
        .await?
        .first()
        .map(|source| object_of(state, source)))
}

/// What the phone sent, in k-vet's fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Incoming {
    pub uid: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    /// A number marked as a mobile one.
    pub mobile: Option<String>,
    /// The first other number.
    pub phone: Option<String>,
    pub emails: Vec<String>,
    pub street: Option<String>,
    pub zip: Option<String>,
    pub city: Option<String>,
    /// An ISO 3166-1 alpha-2 code, when the address names its country by one.
    pub country: Option<String>,
}

/// A value as text; components joined the way they were written.
fn text_of(value: &VCardValue) -> Option<String> {
    match value {
        VCardValue::Text(text) => Some(text.clone()),
        VCardValue::Component(parts) => Some(parts.join(",")),
        _ => None,
    }
}

fn component(entry: &VCardEntry, index: usize) -> Option<String> {
    entry
        .values
        .get(index)
        .and_then(text_of)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn first_text(entry: &VCardEntry) -> Option<String> {
    component(entry, 0)
}

fn is_cell(entry: &VCardEntry) -> bool {
    entry.params.iter().any(|param| {
        param.name == VCardParameterName::Type
            && match &param.value {
                VCardParameterValue::Type(kind) => *kind == VCardType::Cell,
                VCardParameterValue::Text(text) => text.eq_ignore_ascii_case("cell"),
                _ => false,
            }
    })
}

/// "Erika Mustermann (Bello, Minka)" → ("Erika", "Mustermann"): k-vet's own animal list comes
/// off before the name is split.
fn split_shown(shown: &str) -> (Option<String>, Option<String>) {
    let name = match shown.trim().strip_suffix(')') {
        Some(rest) => rest
            .rsplit_once(" (")
            .map_or(shown.trim(), |(name, _)| name),
        None => shown.trim(),
    };
    match name.rsplit_once(' ') {
        Some((first, last)) => (Some(first.trim().to_owned()), Some(last.trim().to_owned())),
        None if name.is_empty() => (None, None),
        None => (None, Some(name.to_owned())),
    }
}

/// Reads a PUT body. `Err` names why it cannot be taken.
pub fn read_incoming(body: &str) -> Result<Incoming, &'static str> {
    let card = VCard::parse(body).map_err(|_| "not a vCard")?;
    let mut incoming = Incoming::default();
    let mut shown = None;
    let mut named = false;
    for entry in &card.entries {
        match &entry.name {
            VCardProperty::Uid => incoming.uid = first_text(entry),
            VCardProperty::Fn => shown = first_text(entry),
            VCardProperty::N => {
                incoming.last_name = component(entry, 0);
                incoming.first_name = component(entry, 1);
                named = incoming.last_name.is_some() || incoming.first_name.is_some();
            }
            VCardProperty::Org => incoming.company = first_text(entry),
            VCardProperty::Tel => {
                let Some(number) = first_text(entry) else {
                    continue;
                };
                let number = number.trim_start_matches("tel:").to_owned();
                if is_cell(entry) && incoming.mobile.is_none() {
                    incoming.mobile = Some(number);
                } else if incoming.phone.is_none() {
                    incoming.phone = Some(number);
                }
            }
            VCardProperty::Email => {
                if let Some(email) = first_text(entry) {
                    incoming.emails.push(email);
                }
            }
            VCardProperty::Adr if incoming.street.is_none() && incoming.city.is_none() => {
                incoming.street = component(entry, 2);
                incoming.city = component(entry, 3);
                incoming.zip = component(entry, 5);
                incoming.country = component(entry, 6)
                    .filter(|country| country.len() == 2)
                    .map(|country| country.to_uppercase());
            }
            _ => {}
        }
    }
    if !named {
        (incoming.first_name, incoming.last_name) = split_shown(shown.as_deref().unwrap_or(""));
    }
    if incoming.last_name.is_none() && incoming.first_name.is_none() && incoming.company.is_none() {
        return Err("a contact without a name");
    }
    Ok(incoming)
}

/// What to store for a number the phone sent: `Some(None)` when it sent none, `Some(e164)` for
/// one that reads, and `None` — keep what is stored — for one that does not.
fn number(raw: Option<&str>, region: &str) -> Option<Option<String>> {
    match raw {
        None => Some(None),
        Some(raw) => validate_phone("phone", raw, region)
            .ok()
            .map(|parsed| Some(parsed.e164)),
    }
}

/// Takes what the phone wrote under `name`.
pub async fn put(
    state: &AppState,
    name: &str,
    body: &str,
    conditions: &Conditions,
) -> Result<Written, WriteError> {
    let incoming = read_incoming(body).map_err(WriteError::Unreadable)?;
    let mut transaction = state.pool.begin().await?;
    let current = object(state, &mut transaction, name).await?;
    if !conditions.hold(current.as_ref()) {
        return Err(WriteError::Precondition);
    }
    let existing = sqlx::query!(
        r#"SELECT id, home_country FROM customer
           WHERE COALESCE(dav_href, 'kvet-customer-' || id || '.vcf') = $1
           FOR UPDATE"#,
        name,
    )
    .fetch_optional(&mut *transaction)
    .await?;

    // A number without a country code is read in the address's country, as in the form.
    let region = incoming
        .country
        .clone()
        .or_else(|| existing.as_ref().and_then(|row| row.home_country.clone()))
        .unwrap_or_else(|| state.config.invoice.default_country.clone());
    let phone = number(incoming.phone.as_deref(), &region);
    let mobile = number(incoming.mobile.as_deref(), &region);

    let (customer_id, written) = match existing {
        // An edit. A complete customer keeps the fields that make it complete: the phone can
        // change them, not empty them.
        Some(row) => {
            sqlx::query!(
                "UPDATE customer SET
                     first_name  = $2,
                     last_name   = CASE WHEN draft THEN $3 ELSE COALESCE($3, last_name) END,
                     company     = $4,
                     phone       = CASE WHEN $11 THEN $5 ELSE phone END,
                     mobile      = CASE WHEN $12 THEN $6 ELSE mobile END,
                     home_street = CASE WHEN draft THEN $7 ELSE COALESCE($7, home_street) END,
                     home_zip    = CASE WHEN draft THEN $8 ELSE COALESCE($8, home_zip) END,
                     home_city   = CASE WHEN draft THEN $9 ELSE COALESCE($9, home_city) END,
                     home_country = COALESCE($10, home_country)
                 WHERE id = $1",
                row.id,
                incoming.first_name,
                incoming.last_name,
                incoming.company,
                phone.clone().flatten(),
                mobile.clone().flatten(),
                incoming.street,
                incoming.zip,
                incoming.city,
                incoming.country,
                phone.is_some(),
                mobile.is_some(),
            )
            .execute(&mut *transaction)
            .await?;
            (row.id, Written::Updated)
        }
        // New: a customer made on the phone — a draft, for want of a salutation at least.
        None => {
            let id: i64 = sqlx::query_scalar!(
                "INSERT INTO customer
                     (first_name, last_name, company, phone, mobile, home_street, home_zip,
                      home_city, home_country, draft, dav_uid, dav_href, dav_origin)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true, $10, $11, 'phone')
                 RETURNING id",
                incoming.first_name,
                incoming.last_name,
                incoming.company,
                phone.flatten(),
                mobile.flatten(),
                incoming.street,
                incoming.zip,
                incoming.city,
                incoming.country,
                incoming
                    .uid
                    .clone()
                    .unwrap_or_else(|| name.trim_end_matches(".vcf").to_owned()),
                name,
            )
            .fetch_one(&mut *transaction)
            .await?;
            (id, Written::Created)
        }
    };

    // The addresses as the phone has them: new ones added, removed ones gone, the others kept
    // with their type. One that is not an address at all is passed over.
    let wanted: Vec<String> = incoming
        .emails
        .iter()
        .filter_map(|email| validate_email("email", email).ok())
        .collect();
    let lowered: HashSet<String> = wanted.iter().map(|email| email.to_lowercase()).collect();
    let stored = sqlx::query!(
        "SELECT id, email FROM customer_email WHERE customer_id = $1",
        customer_id
    )
    .fetch_all(&mut *transaction)
    .await?;
    let kept: HashSet<String> = stored.iter().map(|row| row.email.to_lowercase()).collect();
    for row in &stored {
        if !lowered.contains(&row.email.to_lowercase()) {
            sqlx::query!("DELETE FROM customer_email WHERE id = $1", row.id)
                .execute(&mut *transaction)
                .await?;
        }
    }
    for email in wanted {
        if !kept.contains(&email.to_lowercase()) {
            sqlx::query!(
                "INSERT INTO customer_email (customer_id, email) VALUES ($1, $2)",
                customer_id,
                email,
            )
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await?;
    Ok(written)
}

/// A deletion on the phone is not taken: the customer stays, and so does the contact.
pub async fn delete(
    state: &AppState,
    name: &str,
    conditions: &Conditions,
) -> Result<(), WriteError> {
    let mut connection = state.pool.acquire().await?;
    let current = object(state, &mut connection, name).await?;
    if current.is_none() {
        return Err(WriteError::NotFound);
    }
    if !conditions.hold(current.as_ref()) {
        return Err(WriteError::Precondition);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> ContactSource {
        ContactSource {
            id: 3,
            first_name: Some("Erika".to_owned()),
            last_name: Some("Mustermann".to_owned()),
            company: Some("Hof Mustermann; GbR".to_owned()),
            second_name: Some("Max Mustermann".to_owned()),
            phone: Some("+493012345678".to_owned()),
            mobile: Some("+491711234567".to_owned()),
            emails: vec!["erika@example.com".to_owned()],
            street: Some("Musterweg 5".to_owned()),
            zip: Some("12345".to_owned()),
            city: Some("Musterstadt".to_owned()),
            country: Some("DE".to_owned()),
            animals: vec!["Bello".to_owned(), "Minka".to_owned()],
            animals_with_species: vec!["Bello (Hund)".to_owned(), "Minka (Katze)".to_owned()],
            ..ContactSource::default()
        }
    }

    #[test]
    fn a_contact_shows_the_animals_with_the_name() {
        let vcf = contact_vcf(&source(), "http://pi:8080/customers/3");
        assert!(
            vcf.contains("FN:Erika Mustermann (Bello\\, Minka)\r\n"),
            "{vcf}"
        );
        assert!(vcf.contains("N:Mustermann;Erika;;;\r\n"));
        assert!(vcf.contains("ORG:Hof Mustermann\\; GbR\r\n"));
        assert!(vcf.contains("TEL;TYPE=VOICE:+493012345678\r\n"));
        assert!(vcf.contains("TEL;TYPE=CELL:+491711234567\r\n"));
        assert!(vcf.contains("EMAIL;TYPE=INTERNET:erika@example.com\r\n"));
        assert!(vcf.contains("ADR;TYPE=HOME:;;Musterweg 5;Musterstadt;;12345;DE\r\n"));
        assert!(vcf.contains("UID:kvet-customer-3\r\n"));
        let unfolded = vcf.replace("\r\n ", "");
        assert!(
            unfolded.contains("NOTE:Auch: Max Mustermann\\nBello (Hund)\\, Minka (Katze)\\n"),
            "{unfolded}"
        );
        assert!(!vcf.contains("Labrador"));
        // Every line within 75 octets.
        assert!(vcf.split("\r\n").all(|line| line.len() <= 75), "{vcf}");
        // What k-vet writes, k-vet reads back as the same customer — without "(Bello, Minka)".
        let back = read_incoming(&vcf).expect("our own card reads");
        assert_eq!(back.first_name.as_deref(), Some("Erika"));
        assert_eq!(back.last_name.as_deref(), Some("Mustermann"));
        assert_eq!(back.company.as_deref(), Some("Hof Mustermann; GbR"));
        assert_eq!(back.phone.as_deref(), Some("+493012345678"));
        assert_eq!(back.mobile.as_deref(), Some("+491711234567"));
        assert_eq!(back.emails, vec!["erika@example.com".to_owned()]);
        assert_eq!(back.street.as_deref(), Some("Musterweg 5"));
        assert_eq!(back.zip.as_deref(), Some("12345"));
        assert_eq!(back.city.as_deref(), Some("Musterstadt"));
        assert_eq!(back.country.as_deref(), Some("DE"));
    }

    #[test]
    fn a_company_without_a_person_is_named_by_it() {
        let farm = ContactSource {
            id: 4,
            company: Some("Gut Sonnenhof".to_owned()),
            ..ContactSource::default()
        };
        let vcf = contact_vcf(&farm, "x");
        assert!(vcf.contains("FN:Gut Sonnenhof\r\n"), "{vcf}");
        assert!(!vcf.contains("TEL"));
        assert!(!vcf.contains("ADR"));
    }

    #[test]
    fn a_contact_from_the_phone_reads_in_either_version() {
        let v4 = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:abc\r\nFN:Heike Müller\r\n\
                  N:Müller;Heike;;;\r\nTEL;TYPE=home,voice;VALUE=uri:tel:+49-30-1234567\r\n\
                  TEL;TYPE=cell:0171 1234567\r\nEMAIL:heike@example.org\r\n\
                  ADR;TYPE=home:;;Dorfstraße 1;Neustadt;;54321;Deutschland\r\nEND:VCARD\r\n";
        let incoming = read_incoming(v4).expect("a vCard 4");
        assert_eq!(incoming.uid.as_deref(), Some("urn:uuid:abc"));
        assert_eq!(incoming.last_name.as_deref(), Some("Müller"));
        assert_eq!(incoming.mobile.as_deref(), Some("0171 1234567"));
        assert_eq!(incoming.phone.as_deref(), Some("+49-30-1234567"));
        assert_eq!(incoming.street.as_deref(), Some("Dorfstraße 1"));
        // A country written out is not a code: left as it was.
        assert_eq!(incoming.country, None);

        // Only FN, folded, with k-vet's animal list on it.
        let v3 = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Erika Mustermann (Bello\\, \r\n Minka)\r\n\
                  END:VCARD\r\n";
        let incoming = read_incoming(v3).expect("a vCard 3");
        assert_eq!(incoming.first_name.as_deref(), Some("Erika"));
        assert_eq!(incoming.last_name.as_deref(), Some("Mustermann"));
        assert!(read_incoming("BEGIN:VCARD\r\nVERSION:3.0\r\nEND:VCARD\r\n").is_err());
        assert!(read_incoming("nonsense").is_err());
    }
}
