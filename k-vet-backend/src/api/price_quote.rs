//! Preisauskunft: what a list of positions would cost — for a caller on the phone, before there
//! is any visit.
//!
//! Priced exactly as a treatment's lines are: the catalogue values a line would pin
//! (`treatment_items::catalog_line`), the Wegegeld rule (`travel_distance::travel_price`),
//! `money::line_amounts` per line and `money::vat_summary` over them — so the figure told on the
//! phone is the figure the invoice would show at those factors. Nothing is written: the list
//! lives in the browser.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::travel_distance::{self, KmSuggestion};
use crate::api::treatment_items;
use crate::domain::enums::TreatmentItemKind;
use crate::domain::money;
use crate::error::{AppError, AppResult};

/// More positions than any visit has; a guard, not a rule the vet meets.
const MAX_LINES: usize = 100;
/// `treatment_item.factor` is `NUMERIC(7, 3)`.
const FACTOR_SCALE: u32 = 3;

#[derive(Debug, Deserialize, ToSchema)]
pub struct PriceQuoteRequest {
    pub lines: Vec<QuoteLineInput>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QuoteLineInput {
    pub kind: TreatmentItemKind,
    /// The drug packaging's or the service's id, as the picker gives it.
    pub id: i64,
    pub quantity: Decimal,
    /// Services: the factor in percent (100 = single rate); absent, the catalogue's. Drug lines
    /// have none.
    pub factor: Option<Decimal>,
    /// Wegegeld: kilometres one way; absent, the line costs the catalogue price.
    pub km: Option<Decimal>,
    /// Wegegeld: the factor for adverse travel conditions (1–3).
    pub km_multiplier: Option<Decimal>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PriceQuote {
    /// In the order asked.
    pub lines: Vec<QuoteLine>,
    /// One entry per VAT rate, highest first — as the invoice lists them.
    pub vat_groups: Vec<QuoteVatGroup>,
    pub total_net: Decimal,
    pub total_vat: Decimal,
    /// What the caller is told.
    pub total_gross: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteLine {
    pub kind: TreatmentItemKind,
    pub id: i64,
    pub name: String,
    pub got_number: Option<String>,
    /// The factor the line is priced at: the one asked for, else the catalogue's.
    pub factor: Option<Decimal>,
    pub vat_percent: Decimal,
    /// The service bills travel expenses: the line takes kilometres.
    pub travel_expenses: bool,
    pub quantity: Decimal,
    pub km: Option<Decimal>,
    pub km_multiplier: Option<Decimal>,
    /// Net per unit before the factor — the catalogue price, or the Wegegeld for `km`.
    pub price_net: Decimal,
    /// Gross per unit after the factor.
    pub price_gross: Decimal,
    pub line_net: Decimal,
    pub line_gross: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteVatGroup {
    pub vat_percent: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
    pub gross: Decimal,
}

#[utoipa::path(
    post,
    operation_id = "priceQuote",
    path = "/api/price-quote",
    tag = "quote",
    request_body = PriceQuoteRequest,
    responses(
        (status = 200, description = "The lines priced and totalled; nothing is stored", body = PriceQuote),
        (status = 422, description = "A position is unknown or incomplete, or a number is out of range")
    )
)]
pub async fn price_quote(
    State(state): State<AppState>,
    Json(body): Json<PriceQuoteRequest>,
) -> AppResult<Json<PriceQuote>> {
    if body.lines.len() > MAX_LINES {
        return Err(AppError::field("lines", "quote.tooManyLines"));
    }
    validate(&body.lines)?;

    let mut connection = state.pool.acquire().await?;
    let mut lines = Vec::with_capacity(body.lines.len());
    for (index, input) in body.lines.iter().enumerate() {
        let catalog = treatment_items::catalog_line(&mut connection, input.kind, input.id)
            .await
            .map_err(|error| at_line(index, error))?;
        // The same rule as a line added to a treatment (`insert_pinned_item`).
        let (km, km_multiplier) = if catalog.travel_expenses {
            (input.km, input.km_multiplier)
        } else {
            (None, None)
        };
        let price_net = match km {
            Some(km) => {
                travel_distance::travel_price(&state.config.travel_expenses, km, km_multiplier)
            }
            None => catalog.price_net,
        };
        let factor = match input.kind {
            // At the scale a line stores it (NUMERIC(7, 3)), so the quote prices the factor the
            // treatment would keep, and says it the same way.
            TreatmentItemKind::Service => input
                .factor
                .map(|mut factor| {
                    factor.rescale(FACTOR_SCALE);
                    factor
                })
                .or(catalog.factor),
            TreatmentItemKind::DrugPackaging => None,
        };
        let amounts = money::line_amounts(price_net, input.quantity, factor, catalog.vat_percent);
        lines.push(QuoteLine {
            kind: input.kind,
            id: input.id,
            name: catalog.name,
            got_number: catalog.got_number,
            factor,
            vat_percent: catalog.vat_percent,
            travel_expenses: catalog.travel_expenses,
            quantity: input.quantity,
            km,
            km_multiplier,
            price_net,
            price_gross: amounts.price_gross,
            line_net: amounts.line_net,
            line_gross: amounts.line_gross,
        });
    }

    let groups = money::vat_summary(
        &lines
            .iter()
            .map(|line| (line.line_net, line.vat_percent))
            .collect::<Vec<_>>(),
    );
    Ok(Json(PriceQuote {
        total_net: groups.iter().map(|group| group.net).sum(),
        total_vat: groups.iter().map(|group| group.vat).sum(),
        total_gross: money::total_gross(&groups),
        vat_groups: groups
            .into_iter()
            .map(|group| QuoteVatGroup {
                vat_percent: group.vat_percent,
                net: group.net,
                vat: group.vat,
                gross: group.gross,
            })
            .collect(),
        lines,
    }))
}

/// The ranges a treatment line holds too: a positive quantity and factor, kilometres not below
/// zero, a positive travel multiplier.
fn validate(lines: &[QuoteLineInput]) -> AppResult<()> {
    for (index, line) in lines.iter().enumerate() {
        let field = |name: &str| format!("lines.{index}.{name}");
        if line.quantity <= Decimal::ZERO {
            return Err(AppError::field(field("quantity"), "value.mustBePositive"));
        }
        if line.factor.is_some_and(|factor| factor <= Decimal::ZERO) {
            return Err(AppError::field(field("factor"), "value.mustBePositive"));
        }
        if line.km.is_some_and(|km| km < Decimal::ZERO) {
            return Err(AppError::field(field("km"), "value.mustNotBeNegative"));
        }
        if line
            .km_multiplier
            .is_some_and(|multiplier| multiplier <= Decimal::ZERO)
        {
            return Err(AppError::field(
                field("km_multiplier"),
                "value.mustBePositive",
            ));
        }
    }
    Ok(())
}

/// Names the line a catalogue refusal is about: `drug_packaging_id` becomes `lines.3.id`.
fn at_line(index: usize, error: AppError) -> AppError {
    match error {
        AppError::Validation(errors) => AppError::Validation(
            errors
                .into_iter()
                .map(|mut error| {
                    error.field = format!("lines.{index}.id");
                    error
                })
                .collect(),
        ),
        other => other,
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QuoteDistanceRequest {
    /// An existing customer: their last Wegegeld km, else Google to their home.
    pub customer_id: Option<i64>,
    /// Otherwise an address typed for a new caller — Google only.
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub zip: String,
    #[serde(default)]
    pub city: String,
    /// ISO 3166-1 alpha-2; absent, `[invoice] default_country`.
    pub country: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteDistance {
    /// `null` when there is nothing to go on: no earlier Wegegeld and no Google key.
    pub km_suggestion: Option<KmSuggestion>,
}

#[utoipa::path(
    post,
    operation_id = "quoteDistance",
    path = "/api/price-quote/distance",
    tag = "quote",
    request_body = QuoteDistanceRequest,
    responses(
        (status = 200, description = "Kilometres for a Wegegeld position of the quote, or why there are none", body = QuoteDistance),
        (status = 404, description = "Unknown customer")
    )
)]
pub async fn quote_distance(
    State(state): State<AppState>,
    Json(body): Json<QuoteDistanceRequest>,
) -> AppResult<Json<QuoteDistance>> {
    let km_suggestion = match body.customer_id {
        Some(customer_id) => {
            travel_distance::suggest_for_customer(&state, customer_id, None).await?
        }
        None => {
            travel_distance::suggest_for_address(
                &state,
                &body.street,
                &body.zip,
                &body.city,
                body.country.as_deref(),
            )
            .await?
        }
    };
    Ok(Json(QuoteDistance { km_suggestion }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/price-quote", post(price_quote))
        .route("/price-quote/distance", post(quote_distance))
}
