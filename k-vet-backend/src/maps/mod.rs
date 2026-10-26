//! Google Maps Platform: the driving distance a Wegegeld line is prefilled with (Routes API) and
//! whether a customer's address exists (Address Validation API).
//!
//! The only outbound HTTP in k-vet, and only with `[maps] api_key` set. What is sent is an
//! address — street, postcode, town, country — never a name. Nothing Google answers is kept
//! except the kilometres the vet bills, which are hers to correct.

use std::time::Duration;

use axum::body::Bytes;
use axum::http::{Request, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Deserialize;
use serde_json::json;

use crate::config::Config;
use crate::error::{AppError, AppResult};

const ROUTES_HOST: &str = "https://routes.googleapis.com";
const ADDRESS_VALIDATION_HOST: &str = "https://addressvalidation.googleapis.com";
/// Asked while the vet waits for a line to appear, so a slow answer is no answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// The Routes API answers only the fields asked for. The geocoding of the two addresses is
/// what tells a house from a town centre.
const ROUTES_FIELD_MASK: &str = "routes.distanceMeters,routes.routeLabels,geocodingResults";
/// How much of an error answer goes into the log: enough for Google's reason, never the key
/// (which travels in a header, not the URL).
const LOGGED_BODY_CHARS: usize = 300;

/// Why Google gave no usable answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapsError {
    /// Google answered, but knows no route to the address.
    NotFound,
    /// No answer: network, timeout, a refused key, or something unreadable. Logged.
    Unavailable,
}

/// An address as Google is asked about it. `country` is an ISO 3166-1 alpha-2 code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub street: String,
    pub zip: String,
    pub city: String,
    pub country: String,
}

impl Address {
    /// `None` unless street, postcode and town are all there — less is no address to drive to.
    pub fn new(street: &str, zip: &str, city: &str, country: &str) -> Option<Self> {
        let (street, zip, city) = (street.trim(), zip.trim(), city.trim());
        if street.is_empty() || zip.is_empty() || city.is_empty() {
            return None;
        }
        Some(Self {
            street: street.to_owned(),
            zip: zip.to_owned(),
            city: city.to_owned(),
            country: country.trim().to_uppercase(),
        })
    }

    /// One line as Google reads it: "Musterweg 5, 12345 Musterstadt, Germany".
    pub fn one_line(&self) -> String {
        match rust_iso3166::from_alpha2(&self.country) {
            Some(country) => format!(
                "{}, {} {}, {}",
                self.street, self.zip, self.city, country.name
            ),
            None => format!("{}, {} {}", self.street, self.zip, self.city),
        }
    }
}

/// The shortest driving route Google found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    /// One way, in whole kilometres (the vet's choice), half to even.
    pub km: Decimal,
    /// Google could place the destination only roughly (a street, a town), so the route may
    /// end at its centre rather than at the house.
    pub approximate: bool,
}

/// A part of an address as the vet types it, for saying which one is in doubt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AddressPart {
    Street,
    HouseNumber,
    Zip,
    City,
}

impl AddressPart {
    /// The part a Google address component stands for; `None` for the ones the vet does not
    /// type (country, district, …).
    fn of_component(component_type: &str) -> Option<Self> {
        match component_type {
            "route" => Some(Self::Street),
            "street_number" => Some(Self::HouseNumber),
            "postal_code" => Some(Self::Zip),
            "locality" | "postal_town" => Some(Self::City),
            _ => None,
        }
    }
}

/// What Google says about an address that was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressVerdict {
    /// Known down to the house, every part confirmed.
    Found,
    /// Known, but written differently — a postcode Google has otherwise, a spelling: this is how
    /// Google has it, and `changed` says which parts differ.
    Corrected {
        street: String,
        zip: String,
        city: String,
        changed: Vec<AddressPart>,
    },
    /// Nothing to correct, but Google could not confirm every part: `doubtful` names them (it may
    /// be empty when Google only says "check it").
    Uncertain { doubtful: Vec<AddressPart> },
    /// Not known down to the house.
    NotFound,
}

/// The client for both APIs. Built only when a key is configured.
pub struct MapsClient {
    http: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    api_key: String,
    routes_url: String,
    address_validation_url: String,
}

impl MapsClient {
    /// `None` without a key: then nothing is ever sent.
    pub fn from_config(config: &Config) -> AppResult<Option<Self>> {
        if !config.maps.enabled() {
            return Ok(None);
        }
        let override_host = config
            .maps
            .endpoint_override
            .as_deref()
            .map(|host| host.trim_end_matches('/'));

        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(CONNECT_TIMEOUT));
        connector.enforce_http(false);
        let tls = HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(rustls::crypto::ring::default_provider())
            .map_err(|error| AppError::internal("configuring TLS for Google Maps", error))?;
        // Google only ever over TLS; plain HTTP is for the fake the tests point it at.
        let tls = if override_host.is_some() {
            tls.https_or_http()
        } else {
            tls.https_only()
        };
        let connector = tls.enable_http1().wrap_connector(connector);

        Ok(Some(Self {
            http: Client::builder(TokioExecutor::new()).build(connector),
            api_key: config.maps.api_key.trim().to_owned(),
            routes_url: format!(
                "{}/directions/v2:computeRoutes",
                override_host.unwrap_or(ROUTES_HOST)
            ),
            address_validation_url: format!(
                "{}/v1:validateAddress",
                override_host.unwrap_or(ADDRESS_VALIDATION_HOST)
            ),
        }))
    }

    /// The shortest driving route from `origin` to `destination`, without live traffic, so the
    /// same two addresses give the same kilometres every time (the vet's choice over the most
    /// fuel-efficient route).
    pub async fn shortest_route(
        &self,
        origin: &Address,
        destination: &Address,
    ) -> Result<Route, MapsError> {
        let body = json!({
            "origin": { "address": origin.one_line() },
            "destination": { "address": destination.one_line() },
            "travelMode": "DRIVE",
            "routingPreference": "TRAFFIC_UNAWARE",
            "requestedReferenceRoutes": ["SHORTER_DISTANCE"],
            "languageCode": "de",
            "regionCode": origin.country.to_lowercase(),
            "units": "METRIC",
        });
        let answer = self
            .post(&self.routes_url, Some(ROUTES_FIELD_MASK), &body)
            .await?;
        parse_routes(&answer)
    }

    /// Whether `address` exists, and how Google writes it.
    pub async fn check_address(&self, address: &Address) -> Result<AddressVerdict, MapsError> {
        let body = json!({
            "address": {
                "regionCode": address.country,
                "postalCode": address.zip,
                "locality": address.city,
                "addressLines": [address.street],
            },
        });
        let answer = self.post(&self.address_validation_url, None, &body).await?;
        parse_address_validation(&answer, address)
    }

    async fn post(
        &self,
        url: &str,
        field_mask: Option<&str>,
        body: &serde_json::Value,
    ) -> Result<Bytes, MapsError> {
        let mut request = Request::post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Goog-Api-Key", &self.api_key);
        if let Some(mask) = field_mask {
            request = request.header("X-Goog-FieldMask", mask);
        }
        let request = request
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|error| unavailable("building the request", &error))?;

        let exchange = async {
            let response = self
                .http
                .request(request)
                .await
                .map_err(|error| unavailable("sending the request", &error))?;
            let status = response.status();
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|error| unavailable("reading the answer", &error))?
                .to_bytes();
            Ok::<(StatusCode, Bytes), MapsError>((status, bytes))
        };
        let (status, bytes) = tokio::time::timeout(REQUEST_TIMEOUT, exchange)
            .await
            .map_err(|_| unavailable("waiting for the answer", &"timed out"))??;

        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            let start: String = text.chars().take(LOGGED_BODY_CHARS).collect();
            tracing::warn!(%status, body = %start, "Google Maps refused the request");
            return Err(MapsError::Unavailable);
        }
        Ok(bytes)
    }
}

fn unavailable(context: &str, error: &dyn std::fmt::Display) -> MapsError {
    tracing::warn!(%error, "Google Maps: {context} failed");
    MapsError::Unavailable
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutesAnswer {
    #[serde(default)]
    routes: Vec<RouteAnswer>,
    geocoding_results: Option<GeocodingResults>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteAnswer {
    /// Left out of the JSON when it is zero, like every proto3 default.
    #[serde(default)]
    distance_meters: u64,
}

#[derive(Deserialize)]
struct GeocodingResults {
    destination: Option<GeocodedWaypoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeocodedWaypoint {
    #[serde(default)]
    partial_match: bool,
    #[serde(default, rename = "type")]
    types: Vec<String>,
}

/// Geocoding types that name a building rather than an area.
const HOUSE_TYPES: [&str; 3] = ["street_address", "premise", "subpremise"];

/// The shortest of the routes Google returned — the default route and the shorter-distance
/// one, which are the same route when Google has nothing shorter.
fn parse_routes(answer: &[u8]) -> Result<Route, MapsError> {
    let answer: RoutesAnswer =
        serde_json::from_slice(answer).map_err(|error| unavailable("reading the route", &error))?;
    let meters = answer
        .routes
        .iter()
        .map(|route| route.distance_meters)
        .min()
        .ok_or(MapsError::NotFound)?;
    let approximate = answer
        .geocoding_results
        .and_then(|results| results.destination)
        .is_some_and(|destination| {
            destination.partial_match
                || !destination
                    .types
                    .iter()
                    .any(|kind| HOUSE_TYPES.contains(&kind.as_str()))
        });
    Ok(Route {
        km: meters_to_km(meters),
        approximate,
    })
}

/// Metres to whole kilometres, a half to the even one: 12.5 km is 12, 13.5 km is 14.
fn meters_to_km(meters: u64) -> Decimal {
    (Decimal::from(meters) / Decimal::from(1000))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
}

#[derive(Deserialize)]
struct ValidationAnswer {
    result: ValidationResult,
}

#[derive(Deserialize)]
struct ValidationResult {
    verdict: Verdict,
    address: ValidatedAddress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Verdict {
    #[serde(default)]
    validation_granularity: String,
    /// Google's own summary: ACCEPT, CONFIRM, CONFIRM_ADD_SUBPREMISES or FIX.
    #[serde(default)]
    possible_next_action: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidatedAddress {
    #[serde(default)]
    address_components: Vec<AddressComponent>,
    postal_address: Option<PostalAddress>,
    #[serde(default)]
    unconfirmed_component_types: Vec<String>,
    #[serde(default)]
    missing_component_types: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressComponent {
    #[serde(default)]
    component_type: String,
    #[serde(default)]
    confirmation_level: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostalAddress {
    #[serde(default)]
    postal_code: String,
    #[serde(default)]
    locality: String,
    #[serde(default)]
    address_lines: Vec<String>,
}

/// Granularities at which Google knows the building itself.
const HOUSE_GRANULARITIES: [&str; 3] = ["SUB_PREMISE", "PREMISE", "PREMISE_PROXIMITY"];

/// Strict on purpose: a postcode Google only finds plausible, or quietly writes otherwise, is
/// said — not passed as found. Still only ever a hint to the vet; nothing is refused.
fn parse_address_validation(answer: &[u8], typed: &Address) -> Result<AddressVerdict, MapsError> {
    let answer: ValidationAnswer = serde_json::from_slice(answer)
        .map_err(|error| unavailable("reading the address check", &error))?;
    let result = answer.result;
    let components = &result.address.address_components;

    let suspicious = components
        .iter()
        .any(|component| component.confirmation_level == "UNCONFIRMED_AND_SUSPICIOUS");
    let missing = result
        .address
        .missing_component_types
        .iter()
        .any(|kind| AddressPart::of_component(kind).is_some());
    if suspicious
        || missing
        || result.verdict.possible_next_action == "FIX"
        || !HOUSE_GRANULARITIES.contains(&result.verdict.validation_granularity.as_str())
    {
        return Ok(AddressVerdict::NotFound);
    }

    // Google's version of the address, whenever it differs — flagged as replaced or not.
    let suggestion = result.address.postal_address.and_then(|postal| {
        let street = postal.address_lines.into_iter().next()?;
        Address::new(
            &street,
            &postal.postal_code,
            &postal.locality,
            &typed.country,
        )
    });
    if let Some(google) = suggestion {
        let changed = differing_parts(&google, typed);
        if !changed.is_empty() {
            return Ok(AddressVerdict::Corrected {
                street: google.street,
                zip: google.zip,
                city: google.city,
                changed,
            });
        }
    }

    let mut doubtful: Vec<AddressPart> = components
        .iter()
        .filter(|component| component.confirmation_level != "CONFIRMED")
        .filter_map(|component| AddressPart::of_component(&component.component_type))
        .chain(
            result
                .address
                .unconfirmed_component_types
                .iter()
                .filter_map(|kind| AddressPart::of_component(kind)),
        )
        .collect();
    // An interpolated house number: the street is known, the house is Google's guess.
    if result.verdict.validation_granularity == "PREMISE_PROXIMITY" {
        doubtful.push(AddressPart::HouseNumber);
    }
    doubtful.sort();
    doubtful.dedup();
    if !doubtful.is_empty() || result.verdict.possible_next_action == "CONFIRM" {
        return Ok(AddressVerdict::Uncertain { doubtful });
    }
    Ok(AddressVerdict::Found)
}

/// The parts in which Google's version differs from what was typed, case and spacing aside.
fn differing_parts(google: &Address, typed: &Address) -> Vec<AddressPart> {
    let norm = |text: &str| {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let mut parts = Vec::new();
    if norm(&google.street) != norm(&typed.street) {
        parts.push(AddressPart::Street);
    }
    if norm(&google.zip) != norm(&typed.zip) {
        parts.push(AddressPart::Zip);
    }
    if norm(&google.city) != norm(&typed.city) {
        parts.push(AddressPart::City);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed() -> Address {
        Address::new("Musterweg 5", "12345", "Musterstadt", "de").unwrap()
    }

    #[test]
    fn an_address_needs_street_postcode_and_town() {
        assert_eq!(Address::new(" ", "12345", "Musterstadt", "DE"), None);
        assert_eq!(Address::new("Musterweg 5", "", "Musterstadt", "DE"), None);
        assert_eq!(Address::new("Musterweg 5", "12345", "  ", "DE"), None);
        assert_eq!(typed().country, "DE");
    }

    #[test]
    fn the_address_line_names_the_country() {
        assert_eq!(
            typed().one_line(),
            "Musterweg 5, 12345 Musterstadt, Germany"
        );
        let austria = Address::new("Hauptplatz 1", "8010", "Graz", "AT").unwrap();
        assert_eq!(austria.one_line(), "Hauptplatz 1, 8010 Graz, Austria");
    }

    #[test]
    fn the_shortest_of_the_returned_routes_is_taken() {
        let answer = br#"{"routes":[
            {"distanceMeters":12437,"routeLabels":["DEFAULT_ROUTE"]},
            {"distanceMeters":11902,"routeLabels":["SHORTER_DISTANCE"]}],
            "geocodingResults":{"destination":{"type":["street_address"]}}}"#;
        let route = parse_routes(answer).unwrap();
        assert_eq!(route.km, Decimal::from(12));
        assert!(!route.approximate);
    }

    #[test]
    fn no_route_is_not_found() {
        assert_eq!(parse_routes(b"{}"), Err(MapsError::NotFound));
        assert_eq!(parse_routes(br#"{"routes":[]}"#), Err(MapsError::NotFound));
        assert_eq!(parse_routes(b"not json"), Err(MapsError::Unavailable));
    }

    #[test]
    fn a_destination_found_only_roughly_is_flagged() {
        let town = br#"{"routes":[{"distanceMeters":8000}],
            "geocodingResults":{"destination":{"type":["locality","political"]}}}"#;
        assert!(parse_routes(town).unwrap().approximate);
        let partial = br#"{"routes":[{"distanceMeters":8000}],
            "geocodingResults":{"destination":{"partialMatch":true,"type":["premise"]}}}"#;
        assert!(parse_routes(partial).unwrap().approximate);
        // Without geocoding results nothing is claimed either way.
        assert!(
            !parse_routes(br#"{"routes":[{"distanceMeters":8000}]}"#)
                .unwrap()
                .approximate
        );
    }

    #[test]
    fn kilometres_are_whole_with_a_half_to_the_even_one() {
        assert_eq!(meters_to_km(11_902), Decimal::from(12));
        assert_eq!(meters_to_km(12_437), Decimal::from(12));
        assert_eq!(meters_to_km(12_500), Decimal::from(12));
        assert_eq!(meters_to_km(12_501), Decimal::from(13));
        assert_eq!(meters_to_km(13_500), Decimal::from(14));
        assert_eq!(meters_to_km(500), Decimal::ZERO);
        assert_eq!(meters_to_km(0), Decimal::ZERO);
        // The same place: proto3 leaves the zero distance out.
        assert_eq!(
            parse_routes(br#"{"routes":[{}]}"#).unwrap().km,
            Decimal::ZERO
        );
    }

    /// Every part Google knows of an address, at one confirmation level.
    fn parts(level: &str) -> String {
        [
            "route",
            "street_number",
            "postal_code",
            "locality",
            "country",
        ]
        .map(|kind| format!(r#"{{"componentType":"{kind}","confirmationLevel":"{level}"}}"#))
        .join(",")
    }

    /// An answer: `verdict` and `address` are extra JSON members, `components` the list.
    fn validation(
        verdict: &str,
        components: &str,
        address: &str,
        zip: &str,
        line: &str,
    ) -> Vec<u8> {
        format!(
            r#"{{"result":{{"verdict":{{{verdict}}},
                "address":{{"addressComponents":[{components}]{address},
                "postalAddress":{{"regionCode":"DE","postalCode":"{zip}",
                "locality":"Musterstadt","addressLines":["{line}"]}}}}}}}}"#
        )
        .into_bytes()
    }

    fn house(components: &str, zip: &str, line: &str) -> Vec<u8> {
        validation(
            r#""validationGranularity":"PREMISE","possibleNextAction":"ACCEPT""#,
            components,
            "",
            zip,
            line,
        )
    }

    #[test]
    fn a_confirmed_house_is_found() {
        let answer = house(&parts("CONFIRMED"), "12345", "Musterweg 5");
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Found)
        );
        // Google writing it in other case or spacing is no correction.
        let answer = house(&parts("CONFIRMED"), "12345", "musterweg  5");
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Found)
        );
    }

    #[test]
    fn a_spelling_google_corrected_is_offered() {
        let misspelled = Address::new("Musterwg 5", "12345", "Musterstadt", "DE").unwrap();
        let answer = house(&parts("CONFIRMED"), "12345", "Musterweg 5");
        assert_eq!(
            parse_address_validation(&answer, &misspelled),
            Ok(AddressVerdict::Corrected {
                street: "Musterweg 5".to_owned(),
                zip: "12345".to_owned(),
                city: "Musterstadt".to_owned(),
                changed: vec![AddressPart::Street],
            })
        );
    }

    #[test]
    fn a_postcode_google_has_otherwise_is_offered_flagged_or_not() {
        let wrong_zip = Address::new("Musterweg 5", "12354", "Musterstadt", "DE").unwrap();
        let corrected = Ok(AddressVerdict::Corrected {
            street: "Musterweg 5".to_owned(),
            zip: "12345".to_owned(),
            city: "Musterstadt".to_owned(),
            changed: vec![AddressPart::Zip],
        });
        let flagged = parts("CONFIRMED").replace(
            r#""postal_code","confirmationLevel":"CONFIRMED""#,
            r#""postal_code","confirmationLevel":"CONFIRMED","replaced":true"#,
        );
        let answer = house(&flagged, "12345", "Musterweg 5");
        assert_eq!(parse_address_validation(&answer, &wrong_zip), corrected);
        // Google writes the right one without saying so: offered all the same.
        let answer = house(&parts("CONFIRMED"), "12345", "Musterweg 5");
        assert_eq!(parse_address_validation(&answer, &wrong_zip), corrected);
    }

    #[test]
    fn a_part_google_only_finds_plausible_is_named() {
        let plausible_zip = parts("CONFIRMED").replace(
            r#""postal_code","confirmationLevel":"CONFIRMED""#,
            r#""postal_code","confirmationLevel":"UNCONFIRMED_BUT_PLAUSIBLE""#,
        );
        let answer = house(&plausible_zip, "12345", "Musterweg 5");
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Uncertain {
                doubtful: vec![AddressPart::Zip]
            })
        );
        // Named only in the address's own list.
        let answer = validation(
            r#""validationGranularity":"PREMISE""#,
            &parts("CONFIRMED"),
            r#","unconfirmedComponentTypes":["postal_code","locality"]"#,
            "12345",
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Uncertain {
                doubtful: vec![AddressPart::Zip, AddressPart::City]
            })
        );
    }

    #[test]
    fn a_house_google_only_places_on_the_street_is_doubtful() {
        let answer = validation(
            r#""validationGranularity":"PREMISE_PROXIMITY","possibleNextAction":"ACCEPT""#,
            &parts("CONFIRMED"),
            "",
            "12345",
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Uncertain {
                doubtful: vec![AddressPart::HouseNumber]
            })
        );
        // Google asking for a check without naming a part.
        let answer = validation(
            r#""validationGranularity":"PREMISE","possibleNextAction":"CONFIRM""#,
            &parts("CONFIRMED"),
            "",
            "12345",
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Uncertain { doubtful: vec![] })
        );
    }

    #[test]
    fn a_house_google_cannot_confirm_is_not_found() {
        let not_found = Ok(AddressVerdict::NotFound);
        let street_only = validation(
            r#""validationGranularity":"ROUTE""#,
            &parts("UNCONFIRMED_BUT_PLAUSIBLE"),
            "",
            "12345",
            "Musterweg 99",
        );
        assert_eq!(parse_address_validation(&street_only, &typed()), not_found);
        let suspicious = parts("CONFIRMED").replace(
            r#""postal_code","confirmationLevel":"CONFIRMED""#,
            r#""postal_code","confirmationLevel":"UNCONFIRMED_AND_SUSPICIOUS""#,
        );
        let answer = house(&suspicious, "12345", "Musterweg 5");
        assert_eq!(parse_address_validation(&answer, &typed()), not_found);
        let fix = validation(
            r#""validationGranularity":"PREMISE","possibleNextAction":"FIX""#,
            &parts("CONFIRMED"),
            "",
            "12345",
            "Musterweg 5",
        );
        assert_eq!(parse_address_validation(&fix, &typed()), not_found);
        let no_house_number = validation(
            r#""validationGranularity":"PREMISE""#,
            &parts("CONFIRMED"),
            r#","missingComponentTypes":["street_number"]"#,
            "12345",
            "Musterweg",
        );
        assert_eq!(
            parse_address_validation(&no_house_number, &typed()),
            not_found
        );
        assert_eq!(
            parse_address_validation(b"{}", &typed()),
            Err(MapsError::Unavailable)
        );
    }
}
