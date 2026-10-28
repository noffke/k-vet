//! The appointments as a calendar on the vet's phone: a CalDAV server at `/dav`, reached over
//! the practice's VPN by a CalDAV sync app (built and tested against DAVx⁵).
//!
//! Only the subset a sync client uses is served: discovery (`/.well-known/caldav` → principal →
//! calendar home → one calendar), `PROPFIND` with depth 0 or 1, the `calendar-query` and
//! `calendar-multiget` reports, and `GET` / `PUT` / `DELETE` on events. No `sync-collection`:
//! the collection's `getctag` and each event's ETag are what the client syncs by.
//!
//! The phone signs in with the k-vet login over HTTP Basic, on every request. A login that
//! checked out is remembered for a few minutes, so a sync does not run argon2 per request.

pub mod calendar;
pub mod xml;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::any;
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::auth::{Refusal, check_login};
use calendar::{CalendarObject, Conditions, WriteError, Written};
use xml::{APPLE_ICAL, CALDAV, CALENDARSERVER, DAV, Prop, Propfind, Report};

/// How long a login that checked out is taken on trust.
const LOGIN_TRUST: Duration = Duration::from_secs(10 * 60);
/// The one calendar's path segment.
const CALENDAR: &str = "praxis";

/// The last good `Authorization` header, hashed, and when it was checked.
#[derive(Debug, Default)]
pub struct Logins {
    last: Mutex<Option<([u8; 32], Instant)>>,
}

impl Logins {
    fn trusted(&self, digest: &[u8; 32]) -> bool {
        self.last.lock().is_ok_and(|last| {
            last.is_some_and(|(known, at)| &known == digest && at.elapsed() < LOGIN_TRUST)
        })
    }

    fn remember(&self, digest: [u8; 32]) {
        if let Ok(mut last) = self.last.lock() {
            *last = Some((digest, Instant::now()));
        }
    }
}

/// The routes, mounted only when `[dav] enabled`.
pub fn routes(state: AppState) -> Router<AppState> {
    let dav = Router::new()
        .route("/dav", any(handle))
        .route("/dav/", any(handle))
        .route("/dav/{*path}", any(handle))
        .layer(middleware::from_fn_with_state(state, require_basic));
    Router::new()
        .route(
            "/.well-known/caldav",
            any(|| async { Redirect::permanent("/dav/") }),
        )
        .merge(dav)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Basic realm="k-vet", charset="UTF-8""#),
        )],
    )
        .into_response()
}

/// HTTP Basic with the k-vet login; `OPTIONS` is answered without one.
async fn require_basic(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    let Some(value) = request.headers().get(header::AUTHORIZATION) else {
        return unauthorized();
    };
    let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    if state.dav_logins.trusted(&digest) {
        return next.run(request).await;
    }
    let credentials = value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()
        })
        .and_then(|decoded| String::from_utf8(decoded).ok());
    let Some((username, password)) = credentials
        .as_deref()
        .and_then(|credentials| credentials.split_once(':'))
    else {
        return unauthorized();
    };
    match check_login(&state, username, password) {
        Ok(()) => {
            state.dav_logins.remember(digest);
            next.run(request).await
        }
        Err(Refusal::Throttled) => StatusCode::TOO_MANY_REQUESTS.into_response(),
        Err(Refusal::Wrong) => unauthorized(),
    }
}

/// What a path under `/dav` names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resource {
    Root,
    Principal,
    CalendarHome,
    Calendar,
    Event(String),
}

/// Percent-decodes a path segment; `None` for one that does not decode to UTF-8.
fn decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes.get(index) {
            Some(b'%') => {
                let hex = segment.get(index + 1..index + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            Some(&byte) => {
                out.push(byte);
                index += 1;
            }
            None => break,
        }
    }
    String::from_utf8(out).ok()
}

/// Encodes a name for an href: everything but the unreserved characters.
fn encode(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn resource(path: &str, username: &str) -> Option<Resource> {
    let rest = path.strip_prefix("/dav")?;
    let segments: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    match segments.as_slice() {
        [] => Some(Resource::Root),
        ["principals", user] if decode(user).as_deref() == Some(username) => {
            Some(Resource::Principal)
        }
        ["calendars"] => Some(Resource::CalendarHome),
        ["calendars", CALENDAR] => Some(Resource::Calendar),
        ["calendars", CALENDAR, name] => decode(name).map(Resource::Event),
        _ => None,
    }
}

fn principal_href(state: &AppState) -> String {
    format!("/dav/principals/{}/", encode(&state.config.auth.username))
}

fn event_href(name: &str) -> String {
    format!("/dav/calendars/{CALENDAR}/{}", encode(name))
}

fn href_of(state: &AppState, resource: &Resource) -> String {
    match resource {
        Resource::Root => "/dav/".to_owned(),
        Resource::Principal => principal_href(state),
        Resource::CalendarHome => "/dav/calendars/".to_owned(),
        Resource::Calendar => format!("/dav/calendars/{CALENDAR}/"),
        Resource::Event(name) => event_href(name),
    }
}

/// Everything a client may ask of each resource, in the order `allprop` lists them.
fn known_props(resource: &Resource) -> Vec<Prop> {
    let mut props = vec![
        Prop::new(DAV, "resourcetype"),
        Prop::new(DAV, "current-user-principal"),
    ];
    match resource {
        Resource::Root | Resource::CalendarHome => {}
        Resource::Principal => props.extend([
            Prop::new(DAV, "displayname"),
            Prop::new(DAV, "principal-URL"),
            Prop::new(CALDAV, "calendar-home-set"),
            Prop::new(CALDAV, "calendar-user-address-set"),
        ]),
        Resource::Calendar => props.extend([
            Prop::new(DAV, "displayname"),
            Prop::new(DAV, "owner"),
            Prop::new(DAV, "current-user-privilege-set"),
            Prop::new(DAV, "supported-report-set"),
            Prop::new(CALDAV, "supported-calendar-component-set"),
            Prop::new(CALDAV, "calendar-description"),
            Prop::new(CALENDARSERVER, "getctag"),
            Prop::new(APPLE_ICAL, "calendar-color"),
        ]),
        Resource::Event(_) => {
            props.extend([Prop::new(DAV, "getetag"), Prop::new(DAV, "getcontenttype")])
        }
    }
    props
}

/// A property's value as XML, or `None` when this resource has no such property.
fn value(
    state: &AppState,
    resource: &Resource,
    prop: &Prop,
    ctag: &str,
    object: Option<&CalendarObject>,
    with_data: bool,
) -> Option<String> {
    let href = |href: String| format!("<d:href>{}</d:href>", xml::escape(&href));
    if prop.is(DAV, "current-user-principal") {
        return Some(href(principal_href(state)));
    }
    if prop.is(DAV, "resourcetype") {
        return Some(
            match resource {
                Resource::Root | Resource::CalendarHome => "<d:collection/>",
                Resource::Principal => "<d:principal/><d:collection/>",
                Resource::Calendar => "<d:collection/><c:calendar/>",
                Resource::Event(_) => "",
            }
            .to_owned(),
        );
    }
    match resource {
        Resource::Principal => {
            if prop.is(DAV, "displayname") {
                Some(xml::escape(&state.config.auth.username))
            } else if prop.is(DAV, "principal-URL") {
                Some(href(principal_href(state)))
            } else if prop.is(CALDAV, "calendar-home-set") {
                Some(href("/dav/calendars/".to_owned()))
            } else if prop.is(CALDAV, "calendar-user-address-set") {
                Some(href(principal_href(state)))
            } else {
                None
            }
        }
        Resource::Calendar => {
            if prop.is(DAV, "displayname") {
                Some(xml::escape(&state.config.dav.calendar_name))
            } else if prop.is(DAV, "owner") {
                Some(href(principal_href(state)))
            } else if prop.is(DAV, "current-user-privilege-set") {
                Some(
                    [
                        "read",
                        "write",
                        "write-content",
                        "write-properties",
                        "bind",
                        "unbind",
                    ]
                    .map(|privilege| format!("<d:privilege><d:{privilege}/></d:privilege>"))
                    .concat(),
                )
            } else if prop.is(DAV, "supported-report-set") {
                Some(
                    ["calendar-multiget", "calendar-query"]
                        .map(|report| {
                            format!(
                                "<d:supported-report><d:report><c:{report}/></d:report>\
                                 </d:supported-report>"
                            )
                        })
                        .concat(),
                )
            } else if prop.is(CALDAV, "supported-calendar-component-set") {
                Some(r#"<c:comp name="VEVENT"/>"#.to_owned())
            } else if prop.is(CALDAV, "calendar-description") {
                Some("Termine aus k-vet".to_owned())
            } else if prop.is(CALENDARSERVER, "getctag") {
                Some(xml::escape(ctag))
            } else if prop.is(APPLE_ICAL, "calendar-color") {
                Some("#9A4A2FFF".to_owned())
            } else {
                None
            }
        }
        Resource::Event(_) => {
            let object = object?;
            if prop.is(DAV, "getetag") {
                Some(xml::escape(&object.etag))
            } else if prop.is(DAV, "getcontenttype") {
                Some("text/calendar; charset=utf-8; component=VEVENT".to_owned())
            } else if prop.is(CALDAV, "calendar-data") && with_data {
                Some(xml::escape(&object.ics))
            } else {
                None
            }
        }
        Resource::Root | Resource::CalendarHome => None,
    }
}

fn response_for(
    state: &AppState,
    resource: &Resource,
    wanted: &Propfind,
    ctag: &str,
    object: Option<&CalendarObject>,
) -> xml::Response {
    let props = match wanted {
        Propfind::All => known_props(resource),
        Propfind::Props(props) => props.clone(),
    };
    let mut response = xml::Response {
        href: href_of(state, resource),
        ..xml::Response::default()
    };
    for prop in props {
        let with_data = matches!(wanted, Propfind::Props(_));
        match value(state, resource, &prop, ctag, object, with_data) {
            Some(value) => response.found.push((prop, value)),
            None => response.missing.push(prop),
        }
    }
    response
}

fn multistatus(responses: &[xml::Response]) -> Response {
    (
        StatusCode::MULTI_STATUS,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/xml; charset=utf-8"),
        )],
        xml::multistatus(responses),
    )
        .into_response()
}

fn method_is(method: &Method, name: &str) -> bool {
    method.as_str().eq_ignore_ascii_case(name)
}

fn conditions(headers: &HeaderMap) -> Conditions {
    let text = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    Conditions {
        if_match: text(header::IF_MATCH),
        if_none_match: text(header::IF_NONE_MATCH),
    }
}

/// One handler for every method under `/dav`, dispatched by method and path.
async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(resource) = resource(uri.path(), &state.config.auth.username) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let body = String::from_utf8_lossy(&body);
    let outcome = if method == Method::OPTIONS {
        Ok(options())
    } else if method_is(&method, "PROPFIND") {
        propfind(&state, &resource, &headers, &body).await
    } else if method_is(&method, "REPORT") {
        report(&state, &resource, &body).await
    } else if method == Method::GET || method == Method::HEAD {
        get(&state, &resource, method == Method::HEAD).await
    } else if method == Method::PUT {
        put(&state, &resource, &headers, &body).await
    } else if method == Method::DELETE {
        delete(&state, &resource, &headers).await
    } else {
        Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())
    };
    outcome.unwrap_or_else(|error| error.into_response())
}

fn options() -> Response {
    (
        StatusCode::OK,
        [
            (
                header::HeaderName::from_static("dav"),
                "1, 3, calendar-access",
            ),
            (
                header::ALLOW,
                "OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, REPORT",
            ),
        ],
    )
        .into_response()
}

async fn propfind(
    state: &AppState,
    resource: &Resource,
    headers: &HeaderMap,
    body: &str,
) -> Result<Response, crate::error::AppError> {
    let Some(wanted) = xml::parse_propfind(body) else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let depth_one = headers
        .get("depth")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|depth| depth.trim() != "0");
    let mut connection = state.pool.acquire().await?;

    let (objects, ctag) = match resource {
        Resource::Calendar => {
            let objects = calendar::objects(state, &mut connection).await?;
            let ctag = calendar::ctag(&objects);
            (objects, ctag)
        }
        Resource::CalendarHome if depth_one => {
            let objects = calendar::objects(state, &mut connection).await?;
            let ctag = calendar::ctag(&objects);
            (Vec::new(), ctag)
        }
        _ => (Vec::new(), String::new()),
    };

    let responses = match resource {
        Resource::Event(name) => {
            let Some(object) = calendar::object(state, &mut connection, name).await? else {
                return Ok(StatusCode::NOT_FOUND.into_response());
            };
            vec![response_for(state, resource, &wanted, "", Some(&object))]
        }
        Resource::Calendar => {
            let mut responses = vec![response_for(state, resource, &wanted, &ctag, None)];
            if depth_one {
                responses.extend(objects.iter().map(|object| {
                    response_for(
                        state,
                        &Resource::Event(object.name.clone()),
                        &wanted,
                        "",
                        Some(object),
                    )
                }));
            }
            responses
        }
        Resource::CalendarHome => {
            let mut responses = vec![response_for(state, resource, &wanted, "", None)];
            if depth_one {
                responses.push(response_for(
                    state,
                    &Resource::Calendar,
                    &wanted,
                    &ctag,
                    None,
                ));
            }
            responses
        }
        Resource::Root | Resource::Principal => {
            vec![response_for(state, resource, &wanted, "", None)]
        }
    };
    Ok(multistatus(&responses))
}

async fn report(
    state: &AppState,
    resource: &Resource,
    body: &str,
) -> Result<Response, crate::error::AppError> {
    if *resource != Resource::Calendar {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let Some(report) = xml::parse_report(body) else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let mut connection = state.pool.acquire().await?;
    let responses = match report {
        Report::Multiget { props, hrefs } => {
            let wanted = Propfind::Props(props);
            let mut responses = Vec::with_capacity(hrefs.len());
            for href in hrefs {
                let path = href
                    .find("/dav/")
                    .and_then(|start| href.get(start..))
                    .unwrap_or(&href);
                let found = match resource_from_href(state, path) {
                    Some(Resource::Event(name)) => {
                        calendar::object(state, &mut connection, &name).await?
                    }
                    _ => None,
                };
                responses.push(match found {
                    Some(object) => response_for(
                        state,
                        &Resource::Event(object.name.clone()),
                        &wanted,
                        "",
                        Some(&object),
                    ),
                    None => xml::Response {
                        href,
                        status: Some("404 Not Found"),
                        ..xml::Response::default()
                    },
                });
            }
            responses
        }
        Report::Query { props, range } => {
            let wanted = Propfind::Props(props);
            calendar::objects(state, &mut connection)
                .await?
                .iter()
                .filter(|object| match (range, object.starts_at) {
                    (Some((from, until)), Some(starts_at)) => {
                        let ends_at = starts_at
                            + chrono::TimeDelta::minutes(i64::from(
                                state.config.dav.duration_minutes,
                            ));
                        from.is_none_or(|from| ends_at > from)
                            && until.is_none_or(|until| starts_at < until)
                    }
                    _ => true,
                })
                .map(|object| {
                    response_for(
                        state,
                        &Resource::Event(object.name.clone()),
                        &wanted,
                        "",
                        Some(object),
                    )
                })
                .collect()
        }
        Report::Unsupported(name) => {
            tracing::info!(report = %name, "unsupported DAV report");
            return Ok(StatusCode::FORBIDDEN.into_response());
        }
    };
    Ok(multistatus(&responses))
}

fn resource_from_href(state: &AppState, path: &str) -> Option<Resource> {
    resource(path, &state.config.auth.username)
}

async fn get(
    state: &AppState,
    resource: &Resource,
    head: bool,
) -> Result<Response, crate::error::AppError> {
    let Resource::Event(name) = resource else {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    };
    let mut connection = state.pool.acquire().await?;
    let Some(object) = calendar::object(state, &mut connection, name).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let etag =
        HeaderValue::from_str(&object.etag).unwrap_or_else(|_| HeaderValue::from_static("\"\""));
    let body = if head {
        Body::empty()
    } else {
        Body::from(object.ics)
    };
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/calendar; charset=utf-8"),
            ),
            (header::ETAG, etag),
        ],
        body,
    )
        .into_response())
}

fn write_failed(error: WriteError) -> Response {
    match error {
        WriteError::Precondition => StatusCode::PRECONDITION_FAILED.into_response(),
        WriteError::NotFound => StatusCode::NOT_FOUND.into_response(),
        WriteError::Unreadable(reason) => {
            tracing::info!(%reason, "calendar object refused");
            StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response()
        }
        WriteError::App(error) => error.into_response(),
    }
}

/// No ETag in the answer, ever: what k-vet keeps is not the text the phone sent (RFC 4791
/// §5.3.4), so the phone fetches k-vet's version back.
async fn put(
    state: &AppState,
    resource: &Resource,
    headers: &HeaderMap,
    body: &str,
) -> Result<Response, crate::error::AppError> {
    let Resource::Event(name) = resource else {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    };
    Ok(
        match calendar::put(state, name, body, &conditions(headers)).await {
            Ok(Written::Created) => StatusCode::CREATED.into_response(),
            Ok(Written::Updated) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => write_failed(error),
        },
    )
}

async fn delete(
    state: &AppState,
    resource: &Resource,
    headers: &HeaderMap,
) -> Result<Response, crate::error::AppError> {
    let Resource::Event(name) = resource else {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    };
    Ok(
        match calendar::delete(state, name, &conditions(headers)).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => write_failed(error),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_name_the_resources() {
        assert_eq!(resource("/dav", "tierarzt"), Some(Resource::Root));
        assert_eq!(resource("/dav/", "tierarzt"), Some(Resource::Root));
        assert_eq!(
            resource("/dav/principals/tierarzt/", "tierarzt"),
            Some(Resource::Principal)
        );
        assert_eq!(resource("/dav/principals/jemand/", "tierarzt"), None);
        assert_eq!(
            resource("/dav/calendars/", "tierarzt"),
            Some(Resource::CalendarHome)
        );
        assert_eq!(
            resource("/dav/calendars/praxis", "tierarzt"),
            Some(Resource::Calendar)
        );
        assert_eq!(
            resource("/dav/calendars/praxis/a%40b.ics", "tierarzt"),
            Some(Resource::Event("a@b.ics".to_owned()))
        );
        assert_eq!(resource("/dav/calendars/andere/", "tierarzt"), None);
        assert_eq!(encode("a@b c.ics"), "a%40b%20c.ics");
        assert_eq!(decode("%zz"), None);
    }
}
