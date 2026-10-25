//! Preisauskünfte: what a list of positions would cost — for a caller on the phone — kept with
//! the customer, and taken into the visit when they come.
//!
//! A quote is saved as it is put together, like every record. It is a draft until it belongs to
//! a customer (migration 0032); drafts stay reachable as "Offene Preisauskünfte" and are cleaned
//! up 28 days after their last change. Each line keeps what it was priced at when last saved:
//! taken into a visit it is billed at that price, and where the catalogue has moved since, the
//! quote says so.
//!
//! Priced exactly as a treatment's lines are — the catalogue values a line would pin
//! (`treatment_items::catalog_line`), the Wegegeld rule (`travel_distance::travel_price`),
//! `money::line_amounts` per line and `money::vat_summary` over them — so the figure told on the
//! phone is the figure the invoice shows at those factors.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveDate, TimeDelta, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::double_option;
use crate::api::travel_distance::{self, KmSuggestion};
use crate::api::treatment_items::{self, Attribution, TreatmentItem};
use crate::config::TravelExpenseConfig;
use crate::domain::enums::TreatmentItemKind;
use crate::domain::money;
use crate::domain::stock;
use crate::error::{AppError, AppResult};

/// More positions than any visit has; a guard, not a rule the vet meets.
const MAX_LINES: usize = 100;
/// `treatment_item.factor` and `price_quote_line.factor` are `NUMERIC(7, 3)`.
const FACTOR_SCALE: u32 = 3;
/// A draft quote — a call nobody attached to a customer — is cleaned up this long after its last
/// change (`jobs::cleanup_abandoned_drafts`).
pub const DRAFT_QUOTE_MAX_AGE_DAYS: i64 = 28;
/// How many position names a list row shows.
const SUMMARY_POSITIONS: usize = 3;
/// More open quotes than a practice leaves lying about; the list shows the newest.
const OPEN_QUOTES_LIMIT: i64 = 50;

#[derive(Debug, Clone, Deserialize, ToSchema)]
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

/// A line priced at today's catalogue.
struct PricedLine {
    kind: TreatmentItemKind,
    id: i64,
    name: String,
    got_number: Option<String>,
    factor: Option<Decimal>,
    vat_percent: Decimal,
    travel_expenses: bool,
    quantity: Decimal,
    km: Option<Decimal>,
    km_multiplier: Option<Decimal>,
    price_net: Decimal,
}

/// Prices one position as a line added to a treatment today would be (`insert_pinned_item`).
async fn price_line(
    connection: &mut PgConnection,
    travel: &TravelExpenseConfig,
    input: &QuoteLineInput,
) -> AppResult<PricedLine> {
    let catalog = treatment_items::catalog_line(connection, input.kind, input.id).await?;
    let (km, km_multiplier) = if catalog.travel_expenses {
        (input.km, input.km_multiplier)
    } else {
        (None, None)
    };
    let price_net = match km {
        Some(km) => travel_distance::travel_price(travel, km, km_multiplier),
        None => catalog.price_net,
    };
    let factor = match input.kind {
        // At the scale a line stores it, so the quote prices the factor the treatment keeps.
        TreatmentItemKind::Service => input
            .factor
            .map(|mut factor| {
                factor.rescale(FACTOR_SCALE);
                factor
            })
            .or(catalog.factor),
        TreatmentItemKind::DrugPackaging => None,
    };
    Ok(PricedLine {
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
    })
}

/// The ranges a treatment line holds too: a positive quantity and factor, kilometres not below
/// zero, a positive travel multiplier.
fn validate(lines: &[QuoteLineInput]) -> AppResult<()> {
    if lines.len() > MAX_LINES {
        return Err(AppError::field("lines", "quote.tooManyLines"));
    }
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

/// An empty or blank text is no text.
fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Quote {
    pub id: i64,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub name: Option<String>,
    pub note: Option<String>,
    /// A new caller's address, for the Wegegeld, until they are a customer.
    pub address_street: Option<String>,
    pub address_zip: Option<String>,
    pub address_city: Option<String>,
    /// No customer yet: an open quote.
    pub draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// An open quote: the day the cleanup takes it, 28 days after its last change.
    pub removed_on: Option<NaiveDate>,
    /// In order; priced as quoted.
    pub lines: Vec<QuoteLine>,
    /// As quoted, one entry per VAT rate, highest first — as the invoice lists them.
    pub vat_groups: Vec<QuoteVatGroup>,
    pub total_net: Decimal,
    pub total_vat: Decimal,
    /// As quoted: what the caller was told.
    pub total_gross: Decimal,
    /// What the same positions cost at today's catalogue.
    pub today_total_gross: Decimal,
    /// A line's price or VAT is no longer what was quoted.
    pub prices_changed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteLine {
    pub kind: TreatmentItemKind,
    pub id: i64,
    /// As quoted.
    pub name: String,
    pub got_number: Option<String>,
    pub factor: Option<Decimal>,
    pub vat_percent: Decimal,
    /// The service bills travel expenses: the line takes kilometres.
    pub travel_expenses: bool,
    pub quantity: Decimal,
    pub km: Option<Decimal>,
    pub km_multiplier: Option<Decimal>,
    /// Net per unit before the factor, as quoted — the catalogue price, or the Wegegeld for `km`.
    pub price_net: Decimal,
    /// Gross per unit after the factor, as quoted.
    pub price_gross: Decimal,
    pub line_net: Decimal,
    pub line_gross: Decimal,
    /// Net per unit before the factor at today's catalogue; absent when the position can no
    /// longer be priced.
    pub today_price_net: Option<Decimal>,
    pub today_vat_percent: Option<Decimal>,
    /// The line at today's catalogue, gross — what the page shows beside the quoted one.
    pub today_line_gross: Option<Decimal>,
    /// Today's price or VAT differs from the quoted one, or the position is gone.
    pub price_changed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteVatGroup {
    pub vat_percent: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
    pub gross: Decimal,
}

/// One quote in a list: the customer's, or the open ones.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteSummary {
    pub id: i64,
    pub customer_id: Option<i64>,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub removed_on: Option<NaiveDate>,
    /// A typed address as one line, for an open quote.
    pub address: Option<String>,
    /// The first positions' names.
    pub positions: Vec<String>,
    pub line_count: i64,
    /// As quoted.
    pub total_gross: Decimal,
    pub prices_changed: bool,
}

impl From<Quote> for QuoteSummary {
    fn from(quote: Quote) -> Self {
        let address = match (
            &quote.address_street,
            &quote.address_zip,
            &quote.address_city,
        ) {
            (Some(street), zip, Some(city)) => Some(match zip {
                Some(zip) => format!("{street}, {zip} {city}"),
                None => format!("{street}, {city}"),
            }),
            _ => None,
        };
        Self {
            id: quote.id,
            customer_id: quote.customer_id,
            name: quote.name,
            created_at: quote.created_at,
            updated_at: quote.updated_at,
            removed_on: quote.removed_on,
            address,
            positions: quote
                .lines
                .iter()
                .take(SUMMARY_POSITIONS)
                .map(|line| line.name.clone())
                .collect(),
            line_count: i64::try_from(quote.lines.len()).unwrap_or(i64::MAX),
            total_gross: quote.total_gross,
            prices_changed: quote.prices_changed,
        }
    }
}

/// A quote with its lines as quoted and as priced today.
async fn load_quote(
    connection: &mut PgConnection,
    travel: &TravelExpenseConfig,
    id: i64,
) -> AppResult<Quote> {
    let quote = sqlx::query!(
        r#"SELECT quote.id, quote.customer_id,
                  NULLIF(concat_ws(' ', customer.first_name, customer.last_name), '') AS customer_name,
                  quote.name, quote.note, quote.address_street, quote.address_zip,
                  quote.address_city, quote.draft, quote.created_at, quote.updated_at
           FROM price_quote quote LEFT JOIN customer ON customer.id = quote.customer_id
           WHERE quote.id = $1"#,
        id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let stored = sqlx::query!(
        r#"SELECT kind AS "kind: TreatmentItemKind", drug_packaging_id, service_id, quantity,
                  factor, km, km_multiplier, name, price_net, vat_percent
           FROM price_quote_line WHERE quote_id = $1 ORDER BY position"#,
        id,
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut lines = Vec::with_capacity(stored.len());
    let mut today_nets = Vec::with_capacity(stored.len());
    for line in stored {
        let reference = line
            .drug_packaging_id
            .or(line.service_id)
            .ok_or_else(|| AppError::Internal("quote line without reference".to_owned()))?;
        let input = QuoteLineInput {
            kind: line.kind,
            id: reference,
            quantity: line.quantity,
            factor: line.factor,
            km: line.km,
            km_multiplier: line.km_multiplier,
        };
        // A position that can no longer be priced is a change too, not a failure of the quote.
        let today = match price_line(connection, travel, &input).await {
            Ok(today) => Some(today),
            Err(AppError::Validation(_)) => None,
            Err(error) => return Err(error),
        };
        let quoted =
            money::line_amounts(line.price_net, line.quantity, line.factor, line.vat_percent);
        let today_amounts = today.as_ref().map(|today| {
            money::line_amounts(
                today.price_net,
                line.quantity,
                today.factor,
                today.vat_percent,
            )
        });
        today_nets.push(match (&today, &today_amounts) {
            (Some(today), Some(amounts)) => (amounts.line_net, today.vat_percent),
            _ => (quoted.line_net, line.vat_percent),
        });
        lines.push(QuoteLine {
            kind: line.kind,
            id: reference,
            name: line.name,
            got_number: today.as_ref().and_then(|today| today.got_number.clone()),
            factor: line.factor,
            vat_percent: line.vat_percent,
            travel_expenses: today.as_ref().is_some_and(|today| today.travel_expenses),
            quantity: line.quantity,
            km: line.km,
            km_multiplier: line.km_multiplier,
            price_net: line.price_net,
            price_gross: quoted.price_gross,
            line_net: quoted.line_net,
            line_gross: quoted.line_gross,
            today_price_net: today.as_ref().map(|today| today.price_net),
            today_vat_percent: today.as_ref().map(|today| today.vat_percent),
            today_line_gross: today_amounts.map(|amounts| amounts.line_gross),
            price_changed: today.as_ref().is_none_or(|today| {
                today.price_net != line.price_net || today.vat_percent != line.vat_percent
            }),
        });
    }

    let groups = money::vat_summary(
        &lines
            .iter()
            .map(|line| (line.line_net, line.vat_percent))
            .collect::<Vec<_>>(),
    );
    let today_groups = money::vat_summary(&today_nets);
    let removed_on = quote.draft.then(|| {
        (quote.updated_at.with_timezone(&Local) + TimeDelta::days(DRAFT_QUOTE_MAX_AGE_DAYS))
            .date_naive()
    });
    Ok(Quote {
        id: quote.id,
        customer_id: quote.customer_id,
        customer_name: quote.customer_name,
        name: quote.name,
        note: quote.note,
        address_street: quote.address_street,
        address_zip: quote.address_zip,
        address_city: quote.address_city,
        draft: quote.draft,
        created_at: quote.created_at,
        updated_at: quote.updated_at,
        removed_on,
        prices_changed: lines.iter().any(|line| line.price_changed),
        total_net: groups.iter().map(|group| group.net).sum(),
        total_vat: groups.iter().map(|group| group.vat).sum(),
        total_gross: money::total_gross(&groups),
        today_total_gross: money::total_gross(&today_groups),
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
    })
}

async fn ensure_customer(connection: &mut PgConnection, customer_id: i64) -> AppResult<()> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM customer WHERE id = $1) AS "exists!""#,
        customer_id
    )
    .fetch_one(&mut *connection)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::field("customer_id", "record.notFound"))
    }
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CreateQuote {
    /// For an existing customer the quote is complete at once; without one it is a draft.
    pub customer_id: Option<i64>,
}

#[utoipa::path(
    post,
    operation_id = "createQuote",
    path = "/api/quotes",
    tag = "quote",
    request_body = CreateQuote,
    responses((status = 200, body = Quote), (status = 422, description = "Unknown customer"))
)]
pub async fn create_quote(
    State(state): State<AppState>,
    Json(body): Json<CreateQuote>,
) -> AppResult<Json<Quote>> {
    let mut connection = state.pool.acquire().await?;
    if let Some(customer_id) = body.customer_id {
        ensure_customer(&mut connection, customer_id).await?;
    }
    let id = sqlx::query_scalar!(
        "INSERT INTO price_quote (customer_id, draft) VALUES ($1, $2) RETURNING id",
        body.customer_id,
        body.customer_id.is_none(),
    )
    .fetch_one(&mut *connection)
    .await?;
    load_quote(&mut connection, &state.config.travel_expenses, id)
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    operation_id = "getQuote",
    path = "/api/quotes/{id}",
    tag = "quote",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Quote), (status = 404))
)]
pub async fn get_quote(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Quote>> {
    let mut connection = state.pool.acquire().await?;
    load_quote(&mut connection, &state.config.travel_expenses, id)
        .await
        .map(Json)
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchQuote {
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub address_street: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub address_zip: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub address_city: Option<Option<String>>,
    /// Attaches the quote to a customer, which completes it; the typed address gives way to
    /// their home. A quote never loses its customer.
    pub customer_id: Option<i64>,
    /// The whole list, in order, re-priced at today's catalogue — a quote being worked on is
    /// current; one left alone keeps what it was quoted at.
    pub lines: Option<Vec<QuoteLineInput>>,
}

#[utoipa::path(
    patch,
    operation_id = "patchQuote",
    path = "/api/quotes/{id}",
    tag = "quote",
    params(("id" = i64, Path,)),
    request_body = PatchQuote,
    responses(
        (status = 200, body = Quote),
        (status = 404),
        (status = 422, description = "A position is unknown or incomplete, a number is out of range, or the customer is unknown")
    )
)]
pub async fn patch_quote(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchQuote>,
) -> AppResult<Json<Quote>> {
    if let Some(lines) = &body.lines {
        validate(lines)?;
    }
    let mut transaction = state.pool.begin().await?;
    sqlx::query_scalar!("SELECT id FROM price_quote WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(customer_id) = body.customer_id {
        ensure_customer(&mut transaction, customer_id).await?;
    }

    let text = |value: &Option<Option<String>>| value.clone().map(non_blank);
    let (name, note) = (text(&body.name), text(&body.note));
    let (street, zip, city) = (
        text(&body.address_street),
        text(&body.address_zip),
        text(&body.address_city),
    );
    // Always written, so `updated_at` moves with every change, lines included: an open quote's
    // 28 days count from the last time anyone worked on it.
    sqlx::query!(
        "UPDATE price_quote SET
             name           = CASE WHEN $2 THEN $3 ELSE name END,
             note           = CASE WHEN $4 THEN $5 ELSE note END,
             address_street = CASE WHEN $12::BIGINT IS NOT NULL THEN NULL
                                   WHEN $6 THEN $7 ELSE address_street END,
             address_zip    = CASE WHEN $12::BIGINT IS NOT NULL THEN NULL
                                   WHEN $8 THEN $9 ELSE address_zip END,
             address_city   = CASE WHEN $12::BIGINT IS NOT NULL THEN NULL
                                   WHEN $10 THEN $11 ELSE address_city END,
             customer_id    = COALESCE($12, customer_id),
             draft          = draft AND $12::BIGINT IS NULL
         WHERE id = $1",
        id,
        name.is_some(),
        name.clone().flatten(),
        note.is_some(),
        note.clone().flatten(),
        street.is_some(),
        street.clone().flatten(),
        zip.is_some(),
        zip.clone().flatten(),
        city.is_some(),
        city.clone().flatten(),
        body.customer_id,
    )
    .execute(&mut *transaction)
    .await?;

    if let Some(lines) = &body.lines {
        let mut priced = Vec::with_capacity(lines.len());
        for (index, input) in lines.iter().enumerate() {
            priced.push(
                price_line(&mut transaction, &state.config.travel_expenses, input)
                    .await
                    .map_err(|error| at_line(index, error))?,
            );
        }
        sqlx::query!("DELETE FROM price_quote_line WHERE quote_id = $1", id)
            .execute(&mut *transaction)
            .await?;
        for (position, line) in (1..).zip(&priced) {
            let (packaging_id, service_id) = match line.kind {
                TreatmentItemKind::DrugPackaging => (Some(line.id), None),
                TreatmentItemKind::Service => (None, Some(line.id)),
            };
            sqlx::query!(
                "INSERT INTO price_quote_line
                     (quote_id, position, kind, drug_packaging_id, service_id, quantity, factor,
                      km, km_multiplier, name, price_net, vat_percent)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                id,
                position,
                line.kind as TreatmentItemKind,
                packaging_id,
                service_id,
                line.quantity,
                line.factor,
                line.km,
                line.km_multiplier,
                line.name,
                line.price_net,
                line.vat_percent,
            )
            .execute(&mut *transaction)
            .await?;
        }
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    load_quote(&mut connection, &state.config.travel_expenses, id)
        .await
        .map(Json)
}

#[utoipa::path(
    delete,
    operation_id = "deleteQuote",
    path = "/api/quotes/{id}",
    tag = "quote",
    params(("id" = i64, Path,)),
    responses((status = 204, description = "Deleted, with its lines"), (status = 404))
)]
pub async fn delete_quote(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let deleted = sqlx::query!("DELETE FROM price_quote WHERE id = $1", id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Loads the quotes `ids`, in that order, as list rows.
async fn summaries(state: &AppState, ids: Vec<i64>) -> AppResult<Vec<QuoteSummary>> {
    let mut connection = state.pool.acquire().await?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        rows.push(
            load_quote(&mut connection, &state.config.travel_expenses, id)
                .await?
                .into(),
        );
    }
    Ok(rows)
}

#[utoipa::path(
    get,
    operation_id = "listOpenQuotes",
    path = "/api/quotes/open",
    tag = "quote",
    responses((status = 200, description = "Quotes without a customer, last changed first", body = Vec<QuoteSummary>))
)]
pub async fn list_open_quotes(State(state): State<AppState>) -> AppResult<Json<Vec<QuoteSummary>>> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM price_quote WHERE draft ORDER BY updated_at DESC, id DESC LIMIT $1",
        OPEN_QUOTES_LIMIT,
    )
    .fetch_all(&state.pool)
    .await?;
    summaries(&state, ids).await.map(Json)
}

#[utoipa::path(
    get,
    operation_id = "listCustomerQuotes",
    path = "/api/customers/{id}/quotes",
    tag = "quote",
    params(("id" = i64, Path,)),
    responses((status = 200, description = "The customer's quotes, newest first", body = Vec<QuoteSummary>))
)]
pub async fn list_customer_quotes(
    State(state): State<AppState>,
    Path(customer_id): Path<i64>,
) -> AppResult<Json<Vec<QuoteSummary>>> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM price_quote
         WHERE customer_id = $1 AND NOT draft
         ORDER BY created_at DESC, id DESC",
        customer_id,
    )
    .fetch_all(&state.pool)
    .await?;
    summaries(&state, ids).await.map(Json)
}

/// Which prices a quote's positions are taken into a visit at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuotePrices {
    /// What the caller was told.
    #[default]
    Quoted,
    /// Today's catalogue — for a GOT position whose fee the schedule has raised since.
    Today,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyQuote {
    pub quote_id: i64,
    /// The animal the positions go to; a per-visit service goes to the visit whatever is asked.
    pub patient_treatment_id: Option<i64>,
    #[serde(default)]
    pub prices: QuotePrices,
}

#[utoipa::path(
    post,
    operation_id = "applyQuote",
    path = "/api/treatments/{id}/apply-quote",
    tag = "treatments",
    params(("id" = i64, Path,)),
    request_body = ApplyQuote,
    responses(
        (status = 200, description = "The treatment's lines, the quote's appended", body = Vec<TreatmentItem>),
        (status = 404),
        (status = 409, description = "The treatment's invoice is accepted"),
        (status = 422, description = "The quote is open, belongs to another customer, or a position cannot be added")
    )
)]
pub async fn apply_quote(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ApplyQuote>,
) -> AppResult<Json<Vec<TreatmentItem>>> {
    let mut transaction = state.pool.begin().await?;
    stock::ensure_editable(&mut transaction, id).await?;
    crate::api::treatments::ensure_open(&mut transaction, id).await?;

    let quote = sqlx::query!(
        "SELECT customer_id, draft FROM price_quote WHERE id = $1",
        body.quote_id
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if quote.draft {
        return Err(AppError::field("quote_id", "record.incomplete"));
    }
    let treatment_customer =
        sqlx::query_scalar!("SELECT customer_id FROM treatment WHERE id = $1", id)
            .fetch_one(&mut *transaction)
            .await?;
    if treatment_customer != quote.customer_id {
        return Err(AppError::field("quote_id", "quote.otherCustomer"));
    }

    let lines = sqlx::query!(
        r#"SELECT kind AS "kind: TreatmentItemKind", drug_packaging_id, service_id, quantity,
                  factor, km, km_multiplier, name, price_net, vat_percent
           FROM price_quote_line WHERE quote_id = $1 ORDER BY position"#,
        body.quote_id,
    )
    .fetch_all(&mut *transaction)
    .await?;

    let mut kilometres = false;
    for line in lines {
        let reference = line
            .drug_packaging_id
            .or(line.service_id)
            .ok_or_else(|| AppError::Internal("quote line without reference".to_owned()))?;
        let mut catalog =
            treatment_items::catalog_line(&mut transaction, line.kind, reference).await?;
        if line.kind == TreatmentItemKind::Service {
            catalog.factor = line.factor.or(catalog.factor);
        }
        kilometres |= catalog.travel_expenses && line.km.is_some();
        if body.prices == QuotePrices::Quoted {
            catalog.name = line.name;
            catalog.price_net = line.price_net;
            catalog.vat_percent = line.vat_percent;
            // The quoted price wins: a Wegegeld line is not priced again from its kilometres at
            // today's rates. Its kilometres are kept, and editing them later re-prices it.
            catalog.travel_expenses = false;
        }
        treatment_items::insert_pinned_item(
            &mut transaction,
            id,
            line.kind,
            reference,
            line.quantity,
            body.patient_treatment_id
                .map_or(Attribution::Default, Attribution::Record),
            line.km,
            line.km_multiplier,
            false,
            Some(catalog),
            &state.config.travel_expenses,
        )
        .await?;
    }
    // Kilometres taken over are kilometres entered: the customer's previous visit counts again.
    if kilometres {
        travel_distance::clear_travel_mark(&mut transaction, id).await?;
    }

    transaction.commit().await?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        treatment_items::load_items(&mut connection, id).await?,
    ))
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
        .route("/quotes", post(create_quote))
        .route("/quotes/open", get(list_open_quotes))
        .route(
            "/quotes/{id}",
            get(get_quote).patch(patch_quote).delete(delete_quote),
        )
        .route("/customers/{id}/quotes", get(list_customer_quotes))
        .route("/treatments/{id}/apply-quote", post(apply_quote))
        .route("/price-quote/distance", post(quote_distance))
}
