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
use crate::api::dispense::{self, DispenseRule};
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
    /// A drug, always as its original packaging: the line counts what is dispensed from it, in
    /// its base unit, and bills whole packs and the rest as a Teilmenge (migration 0039).
    DrugPackaging {
        /// The original packaging's id.
        id: i64,
        drug_id: i64,
        /// Drug name; the UI composes the display label with locale formatting.
        name: String,
        unit: Option<String>,
        /// Base units in one pack.
        quantity: Option<Decimal>,
        /// One pack, net; `price_gross` with VAT.
        price_net: Decimal,
        price_gross: Decimal,
        vat_percent: Decimal,
        /// Derived stock over all lots, in base units — a shortfall is visible before picking.
        in_stock: Decimal,
        /// What the drug starts at (`rule.preset`) is more than the lots hold together: picking
        /// it is refused (`item.stockShort`). Never for a drug "Ohne Bestand".
        stock_short: bool,
        /// "Ohne Bestand" (migration 0043): a consumable whose stock is not kept — shown as
        /// such rather than as a stock of 0, and never short.
        without_stock: bool,
        /// How it can be dispensed: whole packs, the Teilmenge's steps, or any amount.
        rule: DispenseRule,
        uses: i64,
    },
    Service {
        id: i64,
        name: String,
        got_number: Option<String>,
        factor: Option<Decimal>,
        /// Net price, before the Steigerungssatz; `price_gross` is what the line will cost.
        price_net: Decimal,
        price_gross: Decimal,
        vat_percent: Decimal,
        travel_expenses: bool,
        /// "pro Termin": the line will belong to the visit, not to the animal it is picked for.
        per_visit: bool,
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
                  original.id             AS id,
                  drug.id                 AS reference_id,
                  drug.name               AS name,
                  original.unit           AS unit,
                  original.quantity       AS quantity,
                  original.sales_price_net AS price_net,
                  drug.vat_percent        AS vat_percent,
                  COALESCE(stock.remaining, 0) AS in_stock,
                  original.list_price_net AS list_price_net,
                  rule.subset_size        AS subset_size,
                  rule.subset_price_net   AS subset_price_net,
                  drug.freely_divisible   AS freely_divisible,
                  drug.human_drug         AS human_drug,
                  drug.without_stock      AS without_stock,
                  drug.not_a_medicine     AS not_a_medicine,
                  NULL::text              AS got_number,
                  NULL::numeric           AS factor,
                  false                   AS travel_expenses,
                  false                   AS per_visit,
                  COALESCE(usage.uses, 0) AS uses,
                  CASE WHEN $1::text IS NULL THEN 0
                       ELSE similarity(drug.name, $1) END AS score
           FROM drug_packaging original
           JOIN drug ON drug.id = original.drug_id
           JOIN drug_dispense_rule rule ON rule.original_id = original.id
           -- Every packaging's uses count for the drug: lines of a Teilmenge from before
           -- migration 0039 included.
           LEFT JOIN (SELECT packaging.drug_id, SUM(usage.uses)::bigint AS uses
                        FROM picker_usage usage
                        JOIN drug_packaging packaging ON packaging.id = usage.item_id
                       WHERE usage.kind = 'drug_packaging'
                       GROUP BY packaging.drug_id) usage ON usage.drug_id = drug.id
           LEFT JOIN stock ON stock.drug_id = drug.id
           WHERE original.kind = 'original'
             AND NOT original.draft AND NOT original.archived
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
                  NULL::numeric     AS list_price_net,
                  NULL::numeric     AS subset_size,
                  NULL::numeric     AS subset_price_net,
                  false             AS freely_divisible,
                  false             AS human_drug,
                  false             AS without_stock,
                  false             AS not_a_medicine,
                  service.got_number AS got_number,
                  service.factor    AS factor,
                  service.travel_expenses AS travel_expenses,
                  service.per_visit AS per_visit,
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

    let pharmacy = &state.config.pharmacy;
    let items = rows
        .into_iter()
        .filter_map(|row| {
            let price_net = row.price_net?;
            let vat_percent = row.vat_percent?;
            // With the Steigerungssatz applied, so the preview matches the line the pick
            // produces — a 150 % GOT position must not advertise the bare catalogue fee.
            let price_gross = money::add_vat(
                money::line_total(price_net, Decimal::ONE, row.factor),
                vat_percent,
            )
            .gross;
            let name = row.name.unwrap_or_default();
            let uses = row.uses.unwrap_or(0);
            match row.kind.as_deref() {
                Some("drug_packaging") => {
                    // A drug that cannot be priced yet (no pack size) is not offered.
                    let pricing = dispense::pricing(
                        &dispense::Catalogue {
                            pack_size: row.quantity,
                            pack_price_net: Some(price_net),
                            list_price_net: row.list_price_net,
                            subset_size: row.subset_size,
                            subset_price_net: row.subset_price_net,
                            freely_divisible: row.freely_divisible.unwrap_or(false),
                            human_drug: row.human_drug.unwrap_or(false),
                            vat_percent: Some(vat_percent),
                            not_a_medicine: row.not_a_medicine.unwrap_or(false),
                        },
                        pharmacy,
                    )?;
                    let rule = DispenseRule::from(&pricing);
                    let in_stock = row.in_stock.unwrap_or_default();
                    let without_stock = row.without_stock.unwrap_or(false);
                    Some(PickerItem::DrugPackaging {
                        id: row.id?,
                        drug_id: row.reference_id?,
                        name,
                        unit: row.unit,
                        quantity: row.quantity,
                        price_net,
                        price_gross,
                        vat_percent,
                        in_stock,
                        stock_short: !without_stock && in_stock < rule.preset,
                        without_stock,
                        rule,
                        uses,
                    })
                }
                Some("service") => Some(PickerItem::Service {
                    id: row.id?,
                    name,
                    got_number: row.got_number,
                    factor: row.factor,
                    price_net,
                    price_gross,
                    vat_percent,
                    travel_expenses: row.travel_expenses.unwrap_or(false),
                    per_visit: row.per_visit.unwrap_or(false),
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
