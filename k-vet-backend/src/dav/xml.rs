//! The WebDAV XML a sync client sends (PROPFIND, REPORT) and the multistatus it gets back.
//!
//! Only what a CalDAV or CardDAV client actually asks of one collection is understood; anything else in a
//! request body is ignored rather than refused, and a property k-vet does not know is answered
//! "404 Not Found" inside the multistatus, which is how WebDAV says "no such property".

use chrono::{DateTime, NaiveDateTime, Utc};

pub const DAV: &str = "DAV:";
pub const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
pub const CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
pub const CALENDARSERVER: &str = "http://calendarserver.org/ns/";
pub const APPLE_ICAL: &str = "http://apple.com/ns/ical/";

/// A property by namespace and local name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub ns: String,
    pub name: String,
}

impl Prop {
    pub fn new(ns: &str, name: &str) -> Self {
        Self {
            ns: ns.to_owned(),
            name: name.to_owned(),
        }
    }

    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }
}

/// What a PROPFIND asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Propfind {
    /// `allprop`, `propname` or an empty body: every property k-vet has.
    All,
    Props(Vec<Prop>),
}

/// A `time-range`'s start and end; either may be open.
pub type TimeRange = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// The REPORTs a sync client uses, from either CalDAV or CardDAV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// The named objects.
    Multiget {
        props: Vec<Prop>,
        hrefs: Vec<String>,
    },
    /// Every object, or the events overlapping a time range.
    Query {
        props: Vec<Prop>,
        range: Option<TimeRange>,
    },
    /// Anything else — `sync-collection` among them, which k-vet does not offer.
    Unsupported(String),
}

fn props_of(node: roxmltree::Node<'_, '_>) -> Vec<Prop> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == "prop")
        .map(|prop| {
            prop.children()
                .filter(roxmltree::Node::is_element)
                .map(|element| {
                    Prop::new(
                        element.tag_name().namespace().unwrap_or_default(),
                        element.tag_name().name(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reads a PROPFIND body. `None` for a body that is not XML.
pub fn parse_propfind(body: &str) -> Option<Propfind> {
    if body.trim().is_empty() {
        return Some(Propfind::All);
    }
    let document = roxmltree::Document::parse(body).ok()?;
    let root = document.root_element();
    let wants_props = root
        .children()
        .any(|child| child.is_element() && child.tag_name().name() == "prop");
    Some(if wants_props {
        Propfind::Props(props_of(root))
    } else {
        Propfind::All
    })
}

/// `20261030T080000Z`, as a CalDAV time range writes it.
fn parse_utc(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
        .ok()
        .map(|naive| naive.and_utc())
}

/// Reads a REPORT body. `None` for a body that is not XML.
pub fn parse_report(body: &str) -> Option<Report> {
    let document = roxmltree::Document::parse(body).ok()?;
    let root = document.root_element();
    let props = props_of(root);
    Some(match root.tag_name().name() {
        "calendar-multiget" | "addressbook-multiget" => Report::Multiget {
            props,
            hrefs: root
                .children()
                .filter(|child| child.is_element() && child.tag_name().name() == "href")
                .filter_map(|href| href.text())
                .map(|href| href.trim().to_owned())
                .collect(),
        },
        "calendar-query" | "addressbook-query" => Report::Query {
            props,
            range: root
                .descendants()
                .find(|node| node.is_element() && node.tag_name().name() == "time-range")
                .map(|range| {
                    (
                        range.attribute("start").and_then(parse_utc),
                        range.attribute("end").and_then(parse_utc),
                    )
                }),
        },
        other => Report::Unsupported(other.to_owned()),
    })
}

/// Escapes text for an XML element or attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

fn prefix(ns: &str) -> Option<&'static str> {
    match ns {
        DAV => Some("d"),
        CALDAV => Some("c"),
        CARDDAV => Some("card"),
        CALENDARSERVER => Some("cs"),
        APPLE_ICAL => Some("a"),
        _ => None,
    }
}

/// `<d:getetag>value</d:getetag>`, or the empty element; `value` is XML already.
fn element(prop: &Prop, value: Option<&str>) -> String {
    let (open, close) = match prefix(&prop.ns) {
        Some(prefix) => (
            format!("{prefix}:{}", prop.name),
            format!("{prefix}:{}", prop.name),
        ),
        None => (
            format!("x:{} xmlns:x=\"{}\"", prop.name, escape(&prop.ns)),
            format!("x:{}", prop.name),
        ),
    };
    match value {
        Some(value) if !value.is_empty() => format!("<{open}>{value}</{close}>"),
        _ => format!("<{open}/>"),
    }
}

/// One `<d:response>` of a multistatus.
#[derive(Debug, Default)]
pub struct Response {
    pub href: String,
    /// Properties with their values (XML already).
    pub found: Vec<(Prop, String)>,
    /// Properties asked for that this resource does not have.
    pub missing: Vec<Prop>,
    /// A whole-resource status instead of properties: `404 Not Found` for a multiget miss.
    pub status: Option<&'static str>,
}

/// The `207 Multi-Status` body.
pub fn multistatus(responses: &[Response]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/" xmlns:a="http://apple.com/ns/ical/">"#,
    );
    for response in responses {
        out.push_str("<d:response><d:href>");
        out.push_str(&escape(&response.href));
        out.push_str("</d:href>");
        if let Some(status) = response.status {
            out.push_str("<d:status>HTTP/1.1 ");
            out.push_str(status);
            out.push_str("</d:status>");
        } else {
            if !response.found.is_empty() {
                out.push_str("<d:propstat><d:prop>");
                for (prop, value) in &response.found {
                    out.push_str(&element(prop, Some(value)));
                }
                out.push_str("</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>");
            }
            if !response.missing.is_empty() {
                out.push_str("<d:propstat><d:prop>");
                for prop in &response.missing {
                    out.push_str(&element(prop, None));
                }
                out.push_str("</d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>");
            }
        }
        out.push_str("</d:response>");
    }
    out.push_str("</d:multistatus>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_propfind_names_its_properties() {
        let body = r#"<?xml version="1.0"?><propfind xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
            <prop><resourcetype/><C:calendar-home-set/><CS:getctag/></prop></propfind>"#;
        assert_eq!(
            parse_propfind(body),
            Some(Propfind::Props(vec![
                Prop::new(DAV, "resourcetype"),
                Prop::new(CALDAV, "calendar-home-set"),
                Prop::new(CALENDARSERVER, "getctag"),
            ]))
        );
        assert_eq!(parse_propfind(""), Some(Propfind::All));
        assert_eq!(
            parse_propfind(r#"<propfind xmlns="DAV:"><allprop/></propfind>"#),
            Some(Propfind::All)
        );
        assert_eq!(parse_propfind("not xml"), None);
    }

    #[test]
    fn a_report_is_a_multiget_or_a_query() {
        let multiget = r#"<C:calendar-multiget xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
            <D:prop><D:getetag/><C:calendar-data/></D:prop>
            <D:href>/dav/calendars/praxis/kvet-1.ics</D:href>
            <D:href> /dav/calendars/praxis/a%40b.ics </D:href></C:calendar-multiget>"#;
        assert_eq!(
            parse_report(multiget),
            Some(Report::Multiget {
                props: vec![
                    Prop::new(DAV, "getetag"),
                    Prop::new(CALDAV, "calendar-data")
                ],
                hrefs: vec![
                    "/dav/calendars/praxis/kvet-1.ics".to_owned(),
                    "/dav/calendars/praxis/a%40b.ics".to_owned(),
                ],
            })
        );
        let query = r#"<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
            <D:prop><D:getetag/></D:prop><C:filter><C:comp-filter name="VCALENDAR">
            <C:comp-filter name="VEVENT"><C:time-range start="20261001T000000Z"/>
            </C:comp-filter></C:comp-filter></C:filter></C:calendar-query>"#;
        let Some(Report::Query { props, range }) = parse_report(query) else {
            panic!("a query");
        };
        assert_eq!(props, vec![Prop::new(DAV, "getetag")]);
        assert_eq!(
            range,
            Some((Some("2026-10-01T00:00:00Z".parse().unwrap()), None))
        );
        let contacts = r#"<CARD:addressbook-multiget xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav">
            <D:prop><D:getetag/><CARD:address-data/></D:prop>
            <D:href>/dav/addressbooks/kunden/kvet-customer-1.vcf</D:href></CARD:addressbook-multiget>"#;
        assert_eq!(
            parse_report(contacts),
            Some(Report::Multiget {
                props: vec![
                    Prop::new(DAV, "getetag"),
                    Prop::new(CARDDAV, "address-data")
                ],
                hrefs: vec!["/dav/addressbooks/kunden/kvet-customer-1.vcf".to_owned()],
            })
        );
        assert_eq!(
            parse_report(r#"<sync-collection xmlns="DAV:"/>"#),
            Some(Report::Unsupported("sync-collection".to_owned()))
        );
    }

    #[test]
    fn a_multistatus_says_what_is_there_and_what_is_not() {
        let xml = multistatus(&[Response {
            href: "/dav/calendars/praxis/".to_owned(),
            found: vec![(Prop::new(DAV, "displayname"), escape("Termine & mehr"))],
            missing: vec![Prop::new("urn:example", "color")],
            status: None,
        }]);
        assert!(xml.contains("<d:displayname>Termine &amp; mehr</d:displayname>"));
        assert!(xml.contains(r#"<x:color xmlns:x="urn:example"/>"#));
        assert!(xml.contains("HTTP/1.1 404 Not Found"));
        assert!(roxmltree::Document::parse(&xml).is_ok(), "{xml}");
    }
}
