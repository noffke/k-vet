//! The appointments as calendar events, and what the phone may change of them.
//!
//! Every event is generated when it is asked for, from the appointment, its customer, its
//! animals and its open "Mitbringen" reminders; its ETag is a hash of that text. So a change in
//! any of them reaches the phone at its next sync without anything stored or kept in step.
//!
//! What the phone writes back is taken narrowly: a new timed event becomes an appointment (its
//! customer chosen in k-vet), a moved event moves its appointment (unless the invoice holds the
//! date), and a deleted one goes — or is only hidden from the calendar when something was
//! recorded on it. Titles and descriptions of k-vet's events are generated and not read back.
//! Entries that cannot be appointments — all-day, recurring — are kept as they were sent.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{
    Calendar, CalendarDateTime, Component, DatePerhapsTime, Event, EventLike, Property,
};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use crate::AppState;
use crate::error::{AppError, AppResult};
use crate::pdf::number_de;

/// One file of the calendar collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarObject {
    /// The last segment of its href: `kvet-12.ics`, or the phone's own name.
    pub name: String,
    /// Quoted, as the `ETag` header carries it.
    pub etag: String,
    pub ics: String,
    /// The event's start, for a `calendar-query` time range; absent for kept entries.
    pub starts_at: Option<DateTime<Utc>>,
}

/// Quoted SHA-256 of the text — the same text always gives the same ETag.
pub fn etag_of(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// An appointment with everything its event shows.
#[derive(Debug, Clone)]
pub struct EventSource {
    pub id: i64,
    pub starts_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub note: Option<String>,
    pub uid: Option<String>,
    pub href: Option<String>,
    pub title: Option<String>,
    pub customer_name: Option<String>,
    pub phone: Option<String>,
    pub street: Option<String>,
    pub zip: Option<String>,
    pub city: Option<String>,
    /// "Bello (Hund)" — the name and the species, never the breed.
    pub animals: Vec<String>,
    /// "2× Metacam · 10 ml – für Bello".
    pub reminders: Vec<String>,
}

impl EventSource {
    pub fn name(&self) -> String {
        self.href
            .clone()
            .unwrap_or_else(|| format!("kvet-{}.ics", self.id))
    }
}

/// The event's iCalendar text. `link` is where the appointment opens in k-vet.
pub fn event_ics(source: &EventSource, duration_minutes: u32, link: &str) -> String {
    let summary = match &source.customer_name {
        Some(customer) if source.animals.is_empty() => customer.clone(),
        Some(customer) => format!("{customer} – {}", source.animals.join(", ")),
        None => source
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Termin".to_owned()),
    };
    let location = match (&source.street, &source.city) {
        (Some(street), Some(city)) => {
            let place = [source.zip.as_deref(), Some(city.as_str())]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Some(format!("{street}, {place}"))
        }
        _ => None,
    };
    let mut description = Vec::new();
    if let Some(phone) = source.phone.as_deref().filter(|phone| !phone.is_empty()) {
        description.push(format!("Tel. {phone}"));
    }
    if let Some(note) = source
        .note
        .as_deref()
        .filter(|note| !note.trim().is_empty())
    {
        description.push(note.trim().to_owned());
    }
    if !source.reminders.is_empty() {
        let mut bring = vec!["Mitbringen:".to_owned()];
        bring.extend(source.reminders.iter().map(|line| format!("– {line}")));
        description.push(bring.join("\n"));
    }
    description.push(link.to_owned());

    let mut event = Event::new();
    event
        .uid(
            &source
                .uid
                .clone()
                .unwrap_or_else(|| format!("kvet-{}", source.id)),
        )
        .timestamp(source.updated_at)
        .summary(&summary)
        .description(&description.join("\n\n"))
        .starts(source.starts_at)
        .ends(source.starts_at + TimeDelta::minutes(i64::from(duration_minutes)))
        .url(link);
    if let Some(location) = &location {
        event.location(location);
    }
    let mut calendar = Calendar::empty();
    calendar
        .append_property(Property::new("VERSION", "2.0"))
        .append_property(Property::new("PRODID", "-//k-vet//Praxis-Termine//DE"))
        .push(event.done());
    calendar.done().to_string()
}

/// "2× Metacam · 10 ml – für Bello".
fn reminder_line(
    quantity: Decimal,
    name: &str,
    size: Option<Decimal>,
    unit: Option<&str>,
    note: Option<&str>,
) -> String {
    let mut line = format!("{}× {name}", number_de(quantity));
    if let Some(size) = size {
        line.push_str(&format!(
            " · {} {}",
            number_de(size),
            unit.unwrap_or_default()
        ));
        line = line.trim_end().to_owned();
    }
    if let Some(note) = note.filter(|note| !note.trim().is_empty()) {
        line.push_str(&format!(" – {}", note.trim()));
    }
    line
}

/// Which appointments `sources` reads.
#[derive(Debug, Default)]
pub struct Selection<'a> {
    /// Only those starting from here on — the calendar's window.
    pub from: Option<DateTime<Utc>>,
    /// Only the one under this name.
    pub name: Option<&'a str>,
}

/// The calendar's appointments: complete, dated, not hidden.
pub async fn sources(
    connection: &mut PgConnection,
    selection: Selection<'_>,
) -> AppResult<Vec<EventSource>> {
    let rows = sqlx::query!(
        r#"SELECT appointment.id, appointment.starts_at AS "starts_at!", appointment.updated_at,
                  appointment.note, appointment.calendar_uid, appointment.calendar_href,
                  appointment.calendar_title,
                  NULLIF(concat_ws(' ', customer.first_name, customer.last_name), '')
                      AS customer_name,
                  customer.phone, customer.home_street, customer.home_zip, customer.home_city,
                  ARRAY(SELECT patient.name || COALESCE(' (' || patient.species || ')', '')
                          FROM patient_treatment
                          JOIN patient ON patient.id = patient_treatment.patient_id
                         WHERE patient_treatment.treatment_id = treatment.id
                           AND patient.name IS NOT NULL
                         ORDER BY patient.name) AS "animals!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           LEFT JOIN customer ON customer.id = appointment.customer_id
           WHERE NOT appointment.draft AND appointment.starts_at IS NOT NULL
             AND NOT appointment.calendar_hidden
             AND ($1::timestamptz IS NULL OR appointment.starts_at >= $1)
             AND ($2::text IS NULL
                  OR COALESCE(appointment.calendar_href, 'kvet-' || appointment.id || '.ics') = $2)
           ORDER BY appointment.starts_at, appointment.id"#,
        selection.from,
        selection.name,
    )
    .fetch_all(&mut *connection)
    .await?;

    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let mut reminders: HashMap<i64, Vec<String>> = HashMap::new();
    for row in sqlx::query!(
        r#"SELECT reminder.appointment_id, reminder.quantity, reminder.note,
                  COALESCE(drug.name, '') AS "name!", packaging.quantity AS size, packaging.unit
           FROM appointment_reminder reminder
           JOIN drug_packaging packaging ON packaging.id = reminder.drug_packaging_id
           JOIN drug ON drug.id = packaging.drug_id
           WHERE reminder.appointment_id = ANY($1) AND reminder.taken_item_id IS NULL
           ORDER BY reminder.appointment_id, reminder.position"#,
        &ids,
    )
    .fetch_all(&mut *connection)
    .await?
    {
        reminders
            .entry(row.appointment_id)
            .or_default()
            .push(reminder_line(
                row.quantity,
                &row.name,
                row.size,
                row.unit.as_deref(),
                row.note.as_deref(),
            ));
    }

    Ok(rows
        .into_iter()
        .map(|row| EventSource {
            id: row.id,
            starts_at: row.starts_at,
            updated_at: row.updated_at,
            note: row.note,
            uid: row.calendar_uid,
            href: row.calendar_href,
            title: row.calendar_title,
            customer_name: row.customer_name,
            phone: row.phone,
            street: row.home_street,
            zip: row.home_zip,
            city: row.home_city,
            animals: row.animals,
            reminders: reminders.remove(&row.id).unwrap_or_default(),
        })
        .collect())
}

fn link(state: &AppState, id: i64) -> String {
    format!(
        "{}/appointments/{id}",
        state.config.server.base_url.trim_end_matches('/')
    )
}

fn object_of(state: &AppState, source: &EventSource) -> CalendarObject {
    let ics = event_ics(
        source,
        state.config.dav.duration_minutes,
        &link(state, source.id),
    );
    CalendarObject {
        name: source.name(),
        etag: etag_of(&ics),
        ics,
        starts_at: Some(source.starts_at),
    }
}

/// Every object of the calendar: the window's appointments and the kept entries.
pub async fn objects(
    state: &AppState,
    connection: &mut PgConnection,
) -> AppResult<Vec<CalendarObject>> {
    let from = Utc::now() - TimeDelta::days(i64::from(state.config.dav.past_days));
    let selection = Selection {
        from: Some(from),
        name: None,
    };
    let mut objects: Vec<CalendarObject> = sources(connection, selection)
        .await?
        .iter()
        .map(|source| object_of(state, source))
        .collect();
    for extra in sqlx::query!("SELECT href, ics FROM calendar_extra_event ORDER BY href")
        .fetch_all(&mut *connection)
        .await?
    {
        objects.push(CalendarObject {
            name: extra.href,
            etag: etag_of(&extra.ics),
            ics: extra.ics,
            starts_at: None,
        });
    }
    Ok(objects)
}

/// The object under `name`, wherever it is in time.
pub async fn object(
    state: &AppState,
    connection: &mut PgConnection,
    name: &str,
) -> AppResult<Option<CalendarObject>> {
    let selection = Selection {
        from: None,
        name: Some(name),
    };
    if let Some(source) = sources(connection, selection).await?.first() {
        return Ok(Some(object_of(state, source)));
    }
    let extra = sqlx::query!(
        "SELECT href, ics FROM calendar_extra_event WHERE href = $1",
        name
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(extra.map(|extra| CalendarObject {
        name: extra.href,
        etag: etag_of(&extra.ics),
        ics: extra.ics,
        starts_at: None,
    }))
}

/// A hash over every object's name and ETag: changes whenever anything in the calendar does.
pub fn ctag(objects: &[CalendarObject]) -> String {
    let joined: String = objects
        .iter()
        .map(|object| format!("{}={};", object.name, object.etag))
        .collect();
    etag_of(&joined)
}

/// What the phone sent, read the way k-vet takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A single event at a time: an appointment.
    Timed {
        uid: Option<String>,
        starts_at: DateTime<Utc>,
        summary: Option<String>,
        description: Option<String>,
    },
    /// All-day or recurring: kept verbatim.
    Kept { uid: Option<String> },
}

/// Reads a PUT body. `Err` names why it cannot be taken.
pub fn read_incoming(body: &str, practice: Tz) -> Result<Incoming, &'static str> {
    let calendar = Calendar::from_str(body).map_err(|_| "not an iCalendar object")?;
    let events: Vec<&Event> = calendar.events().collect();
    let event = events.first().ok_or("no VEVENT in it")?;
    let uid = event.get_uid().map(str::to_owned);
    let recurring = events.len() > 1
        || ["RRULE", "RDATE", "RECURRENCE-ID"]
            .iter()
            .any(|key| event.property_value(key).is_some());
    let start = event.get_start().ok_or("an event without DTSTART")?;
    let starts_at = match start {
        DatePerhapsTime::Date(_) => None,
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(moment)) => Some(moment),
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(local)) => practice
            .from_local_datetime(&local)
            .earliest()
            .map(|moment| moment.with_timezone(&Utc)),
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { date_time, tzid }) => {
            let zone = tzid.parse::<Tz>().unwrap_or(practice);
            zone.from_local_datetime(&date_time)
                .earliest()
                .map(|moment| moment.with_timezone(&Utc))
        }
    };
    match starts_at {
        Some(starts_at) if !recurring => Ok(Incoming::Timed {
            uid,
            starts_at,
            summary: event.get_summary().map(str::to_owned),
            description: event.get_description().map(str::to_owned),
        }),
        _ => Ok(Incoming::Kept { uid }),
    }
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
    fn hold(&self, current: Option<&CalendarObject>) -> bool {
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

/// Takes what the phone wrote under `name`.
pub async fn put(
    state: &AppState,
    name: &str,
    body: &str,
    conditions: &Conditions,
) -> Result<Written, WriteError> {
    let practice: Tz = state
        .config
        .dav
        .time_zone
        .parse()
        .unwrap_or(Tz::Europe__Berlin);
    let incoming = read_incoming(body, practice).map_err(WriteError::Unreadable)?;
    let mut transaction = state.pool.begin().await?;
    let current = object(state, &mut transaction, name).await?;
    if !conditions.hold(current.as_ref()) {
        return Err(WriteError::Precondition);
    }

    let appointment = sqlx::query!(
        r#"SELECT appointment.id, appointment.customer_id, appointment.calendar_origin,
                  EXISTS (SELECT 1 FROM invoice
                           WHERE invoice.treatment_id = treatment.id
                             AND invoice.status <> 'cancelled') AS "invoiced!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           WHERE COALESCE(appointment.calendar_href, 'kvet-' || appointment.id || '.ics') = $1
           FOR UPDATE OF appointment"#,
        name,
    )
    .fetch_optional(&mut *transaction)
    .await?;

    let written = match (appointment, incoming) {
        // A k-vet appointment moved on the phone. Its texts are generated and stay so; the
        // invoice, once written, holds the date it bills.
        (
            Some(existing),
            Incoming::Timed {
                starts_at, summary, ..
            },
        ) => {
            if !existing.invoiced {
                sqlx::query!(
                    "UPDATE appointment SET starts_at = $2
                     WHERE id = $1 AND starts_at IS DISTINCT FROM $2",
                    existing.id,
                    starts_at,
                )
                .execute(&mut *transaction)
                .await?;
            }
            // Made on the phone and still without a customer: its title is the phone's.
            if existing.calendar_origin == "phone" && existing.customer_id.is_none() {
                sqlx::query!(
                    "UPDATE appointment SET calendar_title = $2 WHERE id = $1",
                    existing.id,
                    summary.map(|title| title.trim().to_owned()),
                )
                .execute(&mut *transaction)
                .await?;
            }
            Written::Updated
        }
        // Turned into an all-day or recurring entry: an appointment cannot be one, so it stays.
        (Some(_), Incoming::Kept { .. }) => Written::Updated,
        (
            None,
            Incoming::Timed {
                uid,
                starts_at,
                summary,
                description,
            },
        ) => {
            let note = [summary.as_deref(), description.as_deref()]
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let id: i64 = sqlx::query_scalar!(
                "INSERT INTO appointment
                     (starts_at, note, draft, calendar_uid, calendar_href, calendar_origin,
                      calendar_title)
                 VALUES ($1, NULLIF($2, ''), false, $3, $4, 'phone', $5)
                 RETURNING id",
                starts_at,
                note,
                uid.unwrap_or_else(|| name.trim_end_matches(".ics").to_owned()),
                name,
                summary.map(|title| title.trim().to_owned()),
            )
            .fetch_one(&mut *transaction)
            .await?;
            sqlx::query!("INSERT INTO treatment (appointment_id) VALUES ($1)", id)
                .execute(&mut *transaction)
                .await?;
            Written::Created
        }
        (None, Incoming::Kept { uid }) => {
            let created = sqlx::query_scalar!(
                r#"INSERT INTO calendar_extra_event (href, uid, ics) VALUES ($1, $2, $3)
                   ON CONFLICT (href) DO UPDATE SET ics = EXCLUDED.ics, uid = EXCLUDED.uid,
                                                    updated_at = now()
                   RETURNING (xmax = 0) AS "created!""#,
                name,
                uid.unwrap_or_else(|| name.trim_end_matches(".ics").to_owned()),
                body,
            )
            .fetch_one(&mut *transaction)
            .await?;
            if created {
                Written::Created
            } else {
                Written::Updated
            }
        }
    };
    transaction.commit().await?;
    Ok(written)
}

/// Takes a deletion on the phone. An appointment with nothing recorded on it goes; one with
/// animals, lines, reminders or an invoice stays and is only hidden from the calendar.
pub async fn delete(
    state: &AppState,
    name: &str,
    conditions: &Conditions,
) -> Result<(), WriteError> {
    let mut transaction = state.pool.begin().await?;
    let current = object(state, &mut transaction, name).await?;
    if current.is_none() {
        return Err(WriteError::NotFound);
    }
    if !conditions.hold(current.as_ref()) {
        return Err(WriteError::Precondition);
    }
    let appointment = sqlx::query!(
        r#"SELECT appointment.id,
                  (EXISTS (SELECT 1 FROM patient_treatment
                            WHERE patient_treatment.treatment_id = treatment.id)
                   OR EXISTS (SELECT 1 FROM treatment_item
                               WHERE treatment_item.treatment_id = treatment.id)
                   OR EXISTS (SELECT 1 FROM appointment_reminder
                               WHERE appointment_reminder.appointment_id = appointment.id)
                   OR EXISTS (SELECT 1 FROM invoice
                               WHERE invoice.treatment_id = treatment.id)) AS "recorded!"
           FROM appointment
           JOIN treatment ON treatment.appointment_id = appointment.id
           WHERE COALESCE(appointment.calendar_href, 'kvet-' || appointment.id || '.ics') = $1
           FOR UPDATE OF appointment"#,
        name,
    )
    .fetch_optional(&mut *transaction)
    .await?;
    match appointment {
        Some(appointment) if appointment.recorded => {
            sqlx::query!(
                "UPDATE appointment SET calendar_hidden = true WHERE id = $1",
                appointment.id
            )
            .execute(&mut *transaction)
            .await?;
        }
        Some(appointment) => {
            sqlx::query!("DELETE FROM appointment WHERE id = $1", appointment.id)
                .execute(&mut *transaction)
                .await?;
        }
        None => {
            sqlx::query!("DELETE FROM calendar_extra_event WHERE href = $1", name)
                .execute(&mut *transaction)
                .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> EventSource {
        EventSource {
            id: 12,
            starts_at: "2026-10-30T09:30:00Z".parse().unwrap(),
            updated_at: "2026-10-20T08:00:00Z".parse().unwrap(),
            note: Some("Hund ist ängstlich; Tor, hinten".to_owned()),
            uid: None,
            href: None,
            title: None,
            customer_name: Some("Erika Mustermann".to_owned()),
            phone: Some("030 1234567".to_owned()),
            street: Some("Musterweg 5".to_owned()),
            zip: Some("12345".to_owned()),
            city: Some("Musterstadt".to_owned()),
            animals: vec!["Bello (Hund)".to_owned(), "Minka (Katze)".to_owned()],
            reminders: vec![reminder_line(
                Decimal::from(2),
                "Metacam",
                Some(Decimal::new(1000, 2)),
                Some("ml"),
                Some("für Bello"),
            )],
        }
    }

    fn read_back(ics: &str) -> Event {
        let calendar = Calendar::from_str(ics).expect("our own text parses");
        calendar.events().next().expect("one event").clone()
    }

    #[test]
    fn an_event_says_who_where_and_what_to_bring() {
        let ics = event_ics(&source(), 60, "http://pi:8080/appointments/12");
        let event = read_back(&ics);
        assert_eq!(
            event.get_summary(),
            Some("Erika Mustermann – Bello (Hund), Minka (Katze)")
        );
        assert_eq!(event.get_location(), Some("Musterweg 5, 12345 Musterstadt"));
        assert_eq!(event.get_uid(), Some("kvet-12"));
        let description = event.get_description().unwrap_or_default();
        assert!(description.contains("Tel. 030 1234567"), "{description}");
        assert!(
            description.contains("Hund ist ängstlich; Tor, hinten"),
            "{description}"
        );
        assert!(
            description.contains("Mitbringen:\n– 2× Metacam · 10 ml – für Bello"),
            "{description}"
        );
        assert!(description.ends_with("http://pi:8080/appointments/12"));
        assert_eq!(
            event.get_start(),
            Some(DatePerhapsTime::DateTime(CalendarDateTime::Utc(
                "2026-10-30T09:30:00Z".parse().unwrap()
            )))
        );
        assert_eq!(
            event.get_end(),
            Some(DatePerhapsTime::DateTime(CalendarDateTime::Utc(
                "2026-10-30T10:30:00Z".parse().unwrap()
            )))
        );
        // The same appointment, the same text — or the phone would fetch it on every sync.
        assert_eq!(
            ics,
            event_ics(&source(), 60, "http://pi:8080/appointments/12")
        );
    }

    #[test]
    fn without_a_customer_the_phones_title_or_termin() {
        let mut from_phone = source();
        from_phone.customer_name = None;
        from_phone.animals.clear();
        from_phone.title = Some("Müller Hund impfen".to_owned());
        from_phone.uid = Some("abc-123".to_owned());
        let event = read_back(&event_ics(&from_phone, 60, "x"));
        assert_eq!(event.get_summary(), Some("Müller Hund impfen"));
        assert_eq!(event.get_uid(), Some("abc-123"));
        from_phone.title = None;
        let event = read_back(&event_ics(&from_phone, 60, "x"));
        assert_eq!(event.get_summary(), Some("Termin"));
    }

    fn phone_event(start: &str, extra: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:test\r\nBEGIN:VEVENT\r\nUID:abc-123\r\n\
             DTSTAMP:20261020T080000Z\r\n{start}\r\nSUMMARY:Müller Hund impfen\r\n\
             DESCRIPTION:Zweites Tor\\, hinten\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n"
        )
    }

    #[test]
    fn a_timed_event_from_the_phone_is_read_in_its_zone() {
        let berlin = Tz::Europe__Berlin;
        let expected = Incoming::Timed {
            uid: Some("abc-123".to_owned()),
            starts_at: "2026-10-30T07:00:00Z".parse().unwrap(),
            summary: Some("Müller Hund impfen".to_owned()),
            description: Some("Zweites Tor, hinten".to_owned()),
        };
        for start in [
            "DTSTART:20261030T070000Z",
            "DTSTART;TZID=Europe/Berlin:20261030T080000",
            // Floating: the practice's own time.
            "DTSTART:20261030T080000",
        ] {
            assert_eq!(
                read_incoming(&phone_event(start, ""), berlin),
                Ok(expected.clone()),
                "{start}"
            );
        }
    }

    #[test]
    fn all_day_and_recurring_entries_are_kept_as_they_are() {
        let berlin = Tz::Europe__Berlin;
        let kept = Ok(Incoming::Kept {
            uid: Some("abc-123".to_owned()),
        });
        assert_eq!(
            read_incoming(&phone_event("DTSTART;VALUE=DATE:20261030", ""), berlin),
            kept
        );
        assert_eq!(
            read_incoming(
                &phone_event("DTSTART:20261030T070000Z", "RRULE:FREQ=WEEKLY\r\n"),
                berlin
            ),
            kept
        );
        assert!(read_incoming("nonsense", berlin).is_err());
    }

    #[test]
    fn conditions_compare_etags() {
        let current = CalendarObject {
            name: "kvet-1.ics".to_owned(),
            etag: "\"abc\"".to_owned(),
            ics: String::new(),
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
