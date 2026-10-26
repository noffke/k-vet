//! Whether an address that was just typed exists, asked of Google's Address Validation API.
//!
//! A hint for the vet while she enters a customer, never a refusal: new buildings and farms
//! exist that Google does not know. Only the address is sent, and nothing is stored — the
//! answer matters while she is typing, and Google's terms keep its results from being kept.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::error::{AppError, AppResult};
use crate::maps::{Address, AddressPart, AddressVerdict, MapsError};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckAddress {
    pub street: String,
    pub zip: String,
    pub city: String,
    /// ISO 3166-1 alpha-2; absent, `[invoice] default_country`.
    pub country: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AddressCheck {
    pub verdict: AddressCheckVerdict,
    /// `verdict = corrected`: the address as Google writes it.
    pub suggestion: Option<AddressSuggestion>,
    /// `verdict = corrected`: the parts in which Google's version differs.
    pub changed: Vec<AddressCheckPart>,
    /// `verdict = uncertain`: the parts Google could not confirm (may be empty).
    pub doubtful: Vec<AddressCheckPart>,
}

impl AddressCheck {
    fn plain(verdict: AddressCheckVerdict) -> Self {
        Self {
            verdict,
            suggestion: None,
            changed: Vec::new(),
            doubtful: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AddressCheckVerdict {
    /// Known down to the house.
    Found,
    /// Known, written differently — see `suggestion` and `changed`.
    Corrected,
    /// Known, but not every part confirmed — see `doubtful`.
    Uncertain,
    /// Not known down to the house.
    NotFound,
    /// Incomplete, or Google did not answer: nothing to say.
    Unavailable,
}

/// A part of the address as it is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AddressCheckPart {
    Street,
    HouseNumber,
    Zip,
    City,
}

impl From<AddressPart> for AddressCheckPart {
    fn from(part: AddressPart) -> Self {
        match part {
            AddressPart::Street => Self::Street,
            AddressPart::HouseNumber => Self::HouseNumber,
            AddressPart::Zip => Self::Zip,
            AddressPart::City => Self::City,
        }
    }
}

fn parts(parts: Vec<AddressPart>) -> Vec<AddressCheckPart> {
    parts.into_iter().map(AddressCheckPart::from).collect()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AddressSuggestion {
    pub street: String,
    pub zip: String,
    pub city: String,
}

#[utoipa::path(
    post,
    operation_id = "checkAddress",
    path = "/api/address-check",
    tag = "customers",
    request_body = CheckAddress,
    responses(
        (status = 200, body = AddressCheck),
        (status = 422, description = "No Google key configured")
    )
)]
pub async fn check_address(
    State(state): State<AppState>,
    Json(body): Json<CheckAddress>,
) -> AppResult<Json<AddressCheck>> {
    let Some(maps) = state.maps.as_deref() else {
        return Err(AppError::field("street", "route.notConfigured"));
    };
    let country = body
        .country
        .filter(|country| !country.trim().is_empty())
        .unwrap_or_else(|| state.config.invoice.default_country.clone());
    let Some(address) = Address::new(&body.street, &body.zip, &body.city, &country) else {
        return Ok(Json(AddressCheck::plain(AddressCheckVerdict::Unavailable)));
    };

    let check = match maps.check_address(&address).await {
        Ok(AddressVerdict::Found) => AddressCheck::plain(AddressCheckVerdict::Found),
        Ok(AddressVerdict::Corrected {
            street,
            zip,
            city,
            changed,
        }) => AddressCheck {
            verdict: AddressCheckVerdict::Corrected,
            suggestion: Some(AddressSuggestion { street, zip, city }),
            changed: parts(changed),
            doubtful: Vec::new(),
        },
        Ok(AddressVerdict::Uncertain { doubtful }) => AddressCheck {
            doubtful: parts(doubtful),
            ..AddressCheck::plain(AddressCheckVerdict::Uncertain)
        },
        Ok(AddressVerdict::NotFound) | Err(MapsError::NotFound) => {
            AddressCheck::plain(AddressCheckVerdict::NotFound)
        }
        Err(MapsError::Unavailable) => AddressCheck::plain(AddressCheckVerdict::Unavailable),
    };
    Ok(Json(check))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/address-check", post(check_address))
}
