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

/// What Google says about an address that was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressVerdict {
    /// Known down to the house.
    Found,
    /// Known, but spelled or written differently: this is how Google has it.
    Corrected {
        street: String,
        zip: String,
        city: String,
    },
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidatedAddress {
    #[serde(default)]
    address_components: Vec<AddressComponent>,
    postal_address: Option<PostalAddress>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressComponent {
    #[serde(default)]
    confirmation_level: String,
    #[serde(default)]
    spell_corrected: bool,
    #[serde(default)]
    replaced: bool,
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

/// Granularities at which Google has confirmed the building itself.
const HOUSE_GRANULARITIES: [&str; 3] = ["SUB_PREMISE", "PREMISE", "PREMISE_PROXIMITY"];

fn parse_address_validation(answer: &[u8], typed: &Address) -> Result<AddressVerdict, MapsError> {
    let answer: ValidationAnswer = serde_json::from_slice(answer)
        .map_err(|error| unavailable("reading the address check", &error))?;
    let result = answer.result;

    let suspicious = result
        .address
        .address_components
        .iter()
        .any(|component| component.confirmation_level == "UNCONFIRMED_AND_SUSPICIOUS");
    if suspicious || !HOUSE_GRANULARITIES.contains(&result.verdict.validation_granularity.as_str())
    {
        return Ok(AddressVerdict::NotFound);
    }

    let changed = result
        .address
        .address_components
        .iter()
        .any(|component| component.spell_corrected || component.replaced);
    let suggestion = result.address.postal_address.and_then(|postal| {
        let street = postal.address_lines.into_iter().next()?;
        Address::new(
            &street,
            &postal.postal_code,
            &postal.locality,
            &typed.country,
        )
    });
    match suggestion {
        Some(google) if changed && !same_address(&google, typed) => Ok(AddressVerdict::Corrected {
            street: google.street,
            zip: google.zip,
            city: google.city,
        }),
        _ => Ok(AddressVerdict::Found),
    }
}

/// Equal but for case and spacing — a correction that changes neither is no correction.
fn same_address(left: &Address, right: &Address) -> bool {
    let norm = |text: &str| {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(&left.street) == norm(&right.street)
        && norm(&left.zip) == norm(&right.zip)
        && norm(&left.city) == norm(&right.city)
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

    fn validation(granularity: &str, components: &str, lines: &str) -> Vec<u8> {
        format!(
            r#"{{"result":{{"verdict":{{"validationGranularity":"{granularity}"}},
                "address":{{"addressComponents":[{components}],
                "postalAddress":{{"regionCode":"DE","postalCode":"12345",
                "locality":"Musterstadt","addressLines":["{lines}"]}}}}}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn a_confirmed_house_is_found() {
        let answer = validation(
            "PREMISE",
            r#"{"componentType":"street_number","confirmationLevel":"CONFIRMED"}"#,
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Found)
        );
    }

    #[test]
    fn a_spelling_google_corrected_is_offered() {
        let misspelled = Address::new("Musterwg 5", "12345", "Musterstadt", "DE").unwrap();
        let answer = validation(
            "PREMISE",
            r#"{"componentType":"route","confirmationLevel":"CONFIRMED","spellCorrected":true}"#,
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &misspelled),
            Ok(AddressVerdict::Corrected {
                street: "Musterweg 5".to_owned(),
                zip: "12345".to_owned(),
                city: "Musterstadt".to_owned(),
            })
        );
        // A "correction" that only changes case is none.
        let answer = validation(
            "PREMISE",
            r#"{"componentType":"route","confirmationLevel":"CONFIRMED","spellCorrected":true}"#,
            "musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&answer, &typed()),
            Ok(AddressVerdict::Found)
        );
    }

    #[test]
    fn a_house_google_cannot_confirm_is_not_found() {
        let street_only = validation(
            "ROUTE",
            r#"{"componentType":"street_number","confirmationLevel":"UNCONFIRMED_BUT_PLAUSIBLE"}"#,
            "Musterweg 99",
        );
        assert_eq!(
            parse_address_validation(&street_only, &typed()),
            Ok(AddressVerdict::NotFound)
        );
        let suspicious = validation(
            "PREMISE",
            r#"{"componentType":"postal_code","confirmationLevel":"UNCONFIRMED_AND_SUSPICIOUS"}"#,
            "Musterweg 5",
        );
        assert_eq!(
            parse_address_validation(&suspicious, &typed()),
            Ok(AddressVerdict::NotFound)
        );
        assert_eq!(
            parse_address_validation(b"{}", &typed()),
            Err(MapsError::Unavailable)
        );
    }
}
