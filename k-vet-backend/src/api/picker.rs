//! The unified picker behind every drug/service selection (T033, FR-005).
//!
//! One endpoint, one ranked list: fuzzy name search via `pg_trgm`, ordered by how often
//! the entry was used in past treatments (nightly-refreshed `picker_usage`) and then by
//! similarity. Drafts, archived records and hidden services never appear.
//!
//! Without a search term the list shows only what the practice has actually used: an
//! alphabetical slice of the ~900 imported GOT positions would help nobody.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::domain::money;
use crate::error::AppResult;

/// Results are a short list the vet scans, not a page they browse.
const PICKER_LIMIT: i64 = 25;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PickerQuery {
    /// Search term; empty returns the most-used entries.
    pub q: Option<String>,
}

/// A pickable catalog entry — a tagged union so the UI can show distinct icons.
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PickerItem {
    DrugPackaging {
        id: i64,
        drug_id: i64,
        /// Drug name; the UI composes the display label with locale formatting.
        name: String,
        unit: Option<String>,
        quantity: Option<Decimal>,
        /// Net price; `price_gross` is derived from it for display.
        price_net: Decimal,
        price_gross: Decimal,
        vat_percent: Decimal,
        /// Derived stock over all lots — a shortfall is visible before picking.
        in_stock: Decimal,
        uses: i64,
    },
    Service {
        id: i64,
        name: String,
        got_number: Option<String>,
        factor: Option<Decimal>,
        /// Net price; `price_gross` is derived from it for display.
        price_net: Decimal,
        price_gross: Decimal,
        vat_percent: Decimal,
        travel_expenses: bool,
        uses: i64,
    },
}

#[utoipa::path(
    get,
    operation_id = "pickerItems",
    path = "/api/picker/items",
    tag = "picker",
    params(PickerQuery),
    responses((status = 200, body = Vec<PickerItem>))
)]
pub async fn items(
    State(state): State<AppState>,
    Query(query): Query<PickerQuery>,
) -> AppResult<Json<Vec<PickerItem>>> {
    let term = query
        .q
        .as_ref()
        .map(|term| term.trim().to_owned())
        .filter(|term| !term.is_empty());

    let rows = sqlx::query!(
        r#"WITH stock AS (
               -- Stock lives on the original packaging, so it is reported per drug: a
               -- subset entry shows what is actually on the shelf, in base units.
               SELECT drug_id, SUM(remaining) AS remaining
               FROM lot_remaining GROUP BY drug_id
           )
           SELECT 'drug_packaging' AS kind,
                  packaging.id            AS id,
                  drug.id                 AS reference_id,
                  drug.name               AS name,
                  packaging.unit          AS unit,
                  packaging.quantity      AS quantity,
                  packaging.sales_price_net AS price_net,
                  drug.vat_percent        AS vat_percent,
                  COALESCE(stock.remaining, 0) AS in_stock,
                  NULL::text              AS got_number,
                  NULL::numeric           AS factor,
                  false                   AS travel_expenses,
                  COALESCE(usage.uses, 0) AS uses,
                  CASE WHEN $1::text IS NULL THEN 0
                       ELSE similarity(drug.name, $1) END AS score
           FROM drug_packaging packaging
           JOIN drug ON drug.id = packaging.drug_id
           LEFT JOIN picker_usage usage
                  ON usage.kind = 'drug_packaging' AND usage.item_id = packaging.id
           LEFT JOIN stock ON stock.drug_id = packaging.drug_id
           WHERE NOT packaging.draft AND NOT packaging.archived
             AND NOT drug.draft AND NOT drug.archived
             -- No search term: only what the practice has used before.
             AND ($1::text IS NOT NULL OR COALESCE(usage.uses, 0) > 0)
             AND ($1::text IS NULL OR drug.name ILIKE '%' || $1 || '%'
                  OR similarity(drug.name, $1) > 0.2)

           UNION ALL

           SELECT 'service' AS kind,
                  service.id        AS id,
                  service.id        AS reference_id,
                  service.name      AS name,
                  NULL::text        AS unit,
                  NULL::numeric     AS quantity,
                  service.net_price AS price_net,
                  service.vat_percent AS vat_percent,
                  0::numeric        AS in_stock,
                  service.got_number AS got_number,
                  service.factor    AS factor,
                  service.travel_expenses AS travel_expenses,
                  COALESCE(usage.uses, 0) AS uses,
                  CASE WHEN $1::text IS NULL THEN 0
                       ELSE similarity(service.name, $1) END AS score
           FROM service
           LEFT JOIN picker_usage usage
                  ON usage.kind = 'service' AND usage.item_id = service.id
           WHERE NOT service.draft AND NOT service.archived AND NOT service.hidden
             AND ($1::text IS NOT NULL OR COALESCE(usage.uses, 0) > 0)
             AND ($1::text IS NULL OR service.name ILIKE '%' || $1 || '%'
                  OR service.got_number = $1
                  OR similarity(service.name, $1) > 0.2)

           ORDER BY uses DESC, score DESC, name ASC
           LIMIT $2"#,
        term,
        PICKER_LIMIT,
    )
    .fetch_all(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .filter_map(|row| {
            let price_net = row.price_net?;
            let vat_percent = row.vat_percent?;
            let price_gross = money::add_vat(price_net, vat_percent).gross;
            let name = row.name.unwrap_or_default();
            let uses = row.uses.unwrap_or(0);
            match row.kind.as_deref() {
                Some("drug_packaging") => Some(PickerItem::DrugPackaging {
                    id: row.id?,
                    drug_id: row.reference_id?,
                    name,
                    unit: row.unit,
                    quantity: row.quantity,
                    price_net,
                    price_gross,
                    vat_percent,
                    in_stock: row.in_stock.unwrap_or_default(),
                    uses,
                }),
                Some("service") => Some(PickerItem::Service {
                    id: row.id?,
                    name,
                    got_number: row.got_number,
                    factor: row.factor,
                    price_net,
                    price_gross,
                    vat_percent,
                    travel_expenses: row.travel_expenses.unwrap_or(false),
                    uses,
                }),
                _ => None,
            }
        })
        .collect();

    Ok(Json(items))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/picker/items", get(items))
}
