//! Kilometres for a Wegegeld line: taken over from the customer's previous visit, or asked of
//! Google — the shortest driving route from the practice to the customer's home.
//!
//! The previous visit comes first, so a correction the vet made carries forward. A customer
//! whose home address (or the practice's) changed since is marked by the database (migration
//! 0031); for them the next new line asks Google again. The kilometres always stay hers to edit,
//! and lines already written are never touched.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgConnection, Postgres, Transaction};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::treatment_items::{self, TreatmentItem};
use crate::config::TravelExpenseConfig;
use crate::domain::money;
use crate::domain::stock;
use crate::error::{AppError, AppResult};
use crate::maps::{Address, MapsClient, MapsError};

/// Where the kilometres put on a Wegegeld line came from — or why there are none.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct KmSuggestion {
    /// The kilometres put on the line (one way); absent when none could be found.
    pub km: Option<Decimal>,
    pub source: Option<KmSource>,
    /// `source = previous`: when that visit was, if it has a date.
    pub from_visit: Option<DateTime<Utc>>,
    /// `source = route`: Google could place the customer's address only roughly, so the route
    /// may end at the street or the town rather than at the house.
    pub approximate: bool,
    /// Why there are no kilometres.
    pub problem: Option<KmProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KmSource {
    /// The customer's most recent Wegegeld line on another visit.
    Previous,
    /// Google's shortest driving route.
    Route,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KmProblem {
    /// The settings have no complete practice address to drive from.
    NoPracticeAddress,
    /// The customer has no complete home address to drive to.
    NoCustomerAddress,
    /// Google knows no route between the two.
    NotFound,
    /// Google did not answer usably; the log says why.
    Unavailable,
}

impl KmSuggestion {
    fn problem(problem: KmProblem) -> Self {
        Self {
            km: None,
            source: None,
            from_visit: None,
            approximate: false,
            problem: Some(problem),
        }
    }
}

/// A line together with how its kilometres were found, for the requests that look them up.
#[derive(Debug, Serialize, ToSchema)]
pub struct TreatmentItemWithKm {
    #[serde(flatten)]
    pub item: TreatmentItem,
    /// Set when kilometres were looked up for this line.
    pub km_suggestion: Option<KmSuggestion>,
}

/// The customer a treatment drives to.
struct Destination {
    outdated: bool,
    /// A draft customer may have none of these yet.
    street: Option<String>,
    zip: Option<String>,
    city: Option<String>,
    country: Option<String>,
}

/// Whether a service bills travel expenses. An unknown service is not one; adding it fails
/// later with the usual answer.
pub async fn is_travel_service(connection: &mut PgConnection, service_id: i64) -> AppResult<bool> {
    let travel = sqlx::query_scalar!(
        "SELECT travel_expenses FROM service WHERE id = $1",
        service_id
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(travel.unwrap_or(false))
}

/// Kilometres for a new Wegegeld line on `treatment_id`, or `None` when there is nothing to go
/// on — no customer, nothing to take over and no Google key — which leaves the field empty as
/// before.
///
/// Called before the line's transaction opens: Google may take seconds, and nothing should
/// wait on it holding a lock.
pub async fn suggest_km(state: &AppState, treatment_id: i64) -> AppResult<Option<KmSuggestion>> {
    let mut connection = state.pool.acquire().await?;
    let Some((customer_id, destination)) = destination(&mut connection, treatment_id).await? else {
        return Ok(None);
    };

    if !destination.outdated {
        let previous = sqlx::query!(
            r#"SELECT item.km AS "km!", appointment.starts_at
               FROM treatment_item item
               JOIN service ON service.id = item.service_id
               JOIN treatment ON treatment.id = item.treatment_id
               JOIN appointment ON appointment.id = treatment.appointment_id
               WHERE service.travel_expenses AND item.km IS NOT NULL
                 AND treatment.customer_id = $1 AND treatment.id <> $2
               ORDER BY appointment.starts_at DESC NULLS LAST, item.id DESC
               LIMIT 1"#,
            customer_id,
            treatment_id,
        )
        .fetch_optional(&mut *connection)
        .await?;
        if let Some(previous) = previous {
            return Ok(Some(KmSuggestion {
                km: Some(previous.km),
                source: Some(KmSource::Previous),
                from_visit: previous.starts_at,
                approximate: false,
                problem: None,
            }));
        }
    }

    let Some(maps) = state.maps.as_deref() else {
        return Ok(None);
    };
    let origin = practice_address(&mut connection, state).await?;
    // The connection goes back to the pool before Google is asked.
    drop(connection);
    Ok(Some(route(state, maps, origin, &destination).await))
}

/// Asks Google for the shortest route from the practice to the customer.
async fn route(
    state: &AppState,
    maps: &MapsClient,
    origin: Option<Address>,
    destination: &Destination,
) -> KmSuggestion {
    let Some(origin) = origin else {
        return KmSuggestion::problem(KmProblem::NoPracticeAddress);
    };
    let country = destination
        .country
        .as_deref()
        .unwrap_or(&state.config.invoice.default_country);
    let part = |value: &Option<String>| value.clone().unwrap_or_default();
    let Some(target) = Address::new(
        &part(&destination.street),
        &part(&destination.zip),
        &part(&destination.city),
        country,
    ) else {
        return KmSuggestion::problem(KmProblem::NoCustomerAddress);
    };
    match maps.shortest_route(&origin, &target).await {
        Ok(route) => KmSuggestion {
            km: Some(route.km),
            source: Some(KmSource::Route),
            from_visit: None,
            approximate: route.approximate,
            problem: None,
        },
        Err(MapsError::NotFound) => KmSuggestion::problem(KmProblem::NotFound),
        Err(MapsError::Unavailable) => KmSuggestion::problem(KmProblem::Unavailable),
    }
}

/// The treatment's customer and their home address; `None` without a customer.
async fn destination(
    connection: &mut PgConnection,
    treatment_id: i64,
) -> AppResult<Option<(i64, Destination)>> {
    let row = sqlx::query!(
        r#"SELECT customer.id, customer.travel_distance_outdated,
                  customer.home_street, customer.home_zip, customer.home_city, customer.home_country
           FROM treatment JOIN customer ON customer.id = treatment.customer_id
           WHERE treatment.id = $1"#,
        treatment_id,
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row.map(|row| {
        (
            row.id,
            Destination {
                outdated: row.travel_distance_outdated,
                street: row.home_street,
                zip: row.home_zip,
                city: row.home_city,
                country: row.home_country,
            },
        )
    }))
}

/// Where the vet drives from: the practice address of the settings.
async fn practice_address(
    connection: &mut PgConnection,
    state: &AppState,
) -> AppResult<Option<Address>> {
    let row = sqlx::query!(
        "SELECT practice_street, practice_zip, practice_city, practice_country
         FROM global_settings WHERE id"
    )
    .fetch_one(&mut *connection)
    .await?;
    let country = row
        .practice_country
        .unwrap_or_else(|| state.config.invoice.default_country.clone());
    Ok(Address::new(
        &row.practice_street,
        &row.practice_zip,
        &row.practice_city,
        &country,
    ))
}

/// A new distance for the treatment's customer is in — from Google or typed by the vet — so
/// their previous visit counts again from now on.
pub async fn clear_travel_mark(
    transaction: &mut Transaction<'_, Postgres>,
    treatment_id: i64,
) -> AppResult<()> {
    sqlx::query!(
        "UPDATE customer SET travel_distance_outdated = false
         WHERE travel_distance_outdated
           AND id = (SELECT customer_id FROM treatment WHERE id = $1)",
        treatment_id,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// The price of a Wegegeld line for `km`, per GOT § 10 — one rule for adding, editing and
/// recomputing a line.
pub fn travel_price(
    travel: &TravelExpenseConfig,
    km: Decimal,
    multiplier: Option<Decimal>,
) -> Decimal {
    money::travel_expense(km, multiplier, travel.rate_per_double_km, travel.minimum)
}

#[utoipa::path(
    post,
    operation_id = "recomputeTreatmentItemKm",
    path = "/api/treatment-items/{id}/recompute-km",
    tag = "treatments",
    params(("id" = i64, Path,)),
    responses(
        (status = 200, description = "The line, with new kilometres unless `km_suggestion.problem` says why not", body = TreatmentItemWithKm),
        (status = 404),
        (status = 409, description = "The treatment's invoice is accepted"),
        (status = 422, description = "Not a Wegegeld line, or no Google key configured")
    )
)]
pub async fn recompute_km(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<TreatmentItemWithKm>> {
    let Some(maps) = state.maps.clone() else {
        return Err(AppError::field("km", "route.notConfigured"));
    };

    let mut connection = state.pool.acquire().await?;
    let line = sqlx::query!(
        r#"SELECT item.treatment_id, COALESCE(service.travel_expenses, false) AS "travel!"
           FROM treatment_item item LEFT JOIN service ON service.id = item.service_id
           WHERE item.id = $1"#,
        id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;
    if !line.travel {
        return Err(AppError::field("km", "item.notTravel"));
    }
    // Refused before Google is asked, not after.
    stock::ensure_editable(&mut connection, line.treatment_id).await?;

    let suggestion = match destination(&mut connection, line.treatment_id).await? {
        None => KmSuggestion::problem(KmProblem::NoCustomerAddress),
        Some((_, destination)) => {
            let origin = practice_address(&mut connection, &state).await?;
            drop(connection);
            route(&state, &maps, origin, &destination).await
        }
    };

    if let Some(km) = suggestion.km {
        let mut transaction = state.pool.begin().await?;
        let current = sqlx::query!(
            "SELECT treatment_id, km_multiplier FROM treatment_item WHERE id = $1 FOR UPDATE",
            id,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
        stock::ensure_editable(&mut transaction, current.treatment_id).await?;
        let price_net = travel_price(&state.config.travel_expenses, km, current.km_multiplier);
        sqlx::query!(
            "UPDATE treatment_item SET km = $2, price_net = $3 WHERE id = $1",
            id,
            km,
            price_net,
        )
        .execute(&mut *transaction)
        .await?;
        clear_travel_mark(&mut transaction, current.treatment_id).await?;
        transaction.commit().await?;
    }

    let mut connection = state.pool.acquire().await?;
    Ok(Json(TreatmentItemWithKm {
        item: treatment_items::load_item(&mut connection, id).await?,
        km_suggestion: Some(suggestion),
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/treatment-items/{id}/recompute-km", post(recompute_km))
}
