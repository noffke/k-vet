//! What a drug position is since migration 0039: the quantity dispensed, in the original pack's
//! base unit, billed as whole packs plus a Teilmenge (`money::dispense_rows`).
//!
//! Every place a drug quantity is typed — a treatment line, a Behandlungsgruppe, a Preisauskunft,
//! a Mitbringen reminder — reads the drug's rule here (`drug_dispense`), checks the quantity
//! against it (`check`) and reports it with the record (`DispenseRule`), so the page can say "in
//! Schritten von 10 ml" beside the field: a field error carries a key, not values.

use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgConnection;
use utoipa::ToSchema;

use crate::api::drugs::pricing_policy;
use crate::config::PharmacyConfig;
use crate::domain::money::{self, DispensePricing, DispenseRefusal, DispenseRows, SubsetPrice};
use crate::error::{AppError, AppResult};

/// How a drug can be dispensed — what a quantity field shows beside itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct DispenseRule {
    /// Base units in one original pack: 10 (Tbl.), 100 (ml).
    pub pack_size: Decimal,
    /// The smallest Teilmenge: a rest after whole packs goes in multiples of it. Absent, and not
    /// `freely_divisible`: whole packs only.
    pub step: Option<Decimal>,
    /// "Beliebig teilbar": any rest, to two decimals.
    pub freely_divisible: bool,
    /// What a newly picked drug starts at: one unit where it can, else one step, else one pack.
    pub preset: Decimal,
}

impl From<&DispensePricing> for DispenseRule {
    fn from(pricing: &DispensePricing) -> Self {
        Self {
            pack_size: pricing.pack_size,
            step: pricing
                .subset
                .filter(|_| !pricing.freely_divisible)
                .map(|subset| subset.size),
            freely_divisible: pricing.freely_divisible,
            preset: money::dispense_preset(pricing),
        }
    }
}

/// One printed row of a drug line: the whole packs, or the Teilmenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct DispensePart {
    /// Packs on the pack row, base units on the Teilmenge row.
    pub quantity: Decimal,
    /// Gross per pack, or per base unit (to four decimals).
    pub price_gross: Decimal,
    pub line_net: Decimal,
    pub line_gross: Decimal,
}

impl From<&money::DispenseRow> for DispensePart {
    fn from(row: &money::DispenseRow) -> Self {
        Self {
            quantity: row.quantity,
            price_gross: row.price_gross,
            line_net: row.line_net,
            line_gross: row.line_gross,
        }
    }
}

/// A drug line in dispensed base units, split as the invoice prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct Dispense {
    /// As pinned on the line.
    pub rule: DispenseRule,
    /// Whole packs at the pack price; absent below one pack.
    pub packs: Option<DispensePart>,
    /// The rest as a Teilmenge, priced per base unit; absent for whole packs.
    pub remainder: Option<DispensePart>,
}

impl Dispense {
    pub fn new(pricing: &DispensePricing, rows: &DispenseRows) -> Self {
        Self {
            rule: DispenseRule::from(pricing),
            packs: rows.packs.as_ref().map(DispensePart::from),
            remainder: rows.remainder.as_ref().map(DispensePart::from),
        }
    }

    /// The rows that exist, packs first.
    pub fn parts(&self) -> impl Iterator<Item = &DispensePart> {
        self.packs.iter().chain(self.remainder.iter())
    }
}

/// The pricing a line or quote line pinned; `None` for a line in the old form (`pack_size` NULL),
/// whose quantity counts packagings.
pub fn pinned(
    pack_size: Option<Decimal>,
    pack_price_net: Decimal,
    subset_size: Option<Decimal>,
    subset_price_net: Option<Decimal>,
    freely_divisible: bool,
) -> Option<DispensePricing> {
    Some(DispensePricing {
        pack_size: pack_size?,
        pack_price_net,
        subset: subset_size
            .zip(subset_price_net)
            .map(|(size, price_net)| SubsetPrice { size, price_net }),
        freely_divisible,
    })
}

/// A drug's catalogue as `drug_dispense_rule` (migration 0039) gives it.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub pack_size: Option<Decimal>,
    pub pack_price_net: Option<Decimal>,
    pub list_price_net: Option<Decimal>,
    pub subset_size: Option<Decimal>,
    pub subset_price_net: Option<Decimal>,
    pub freely_divisible: bool,
    pub human_drug: bool,
    pub vat_percent: Option<Decimal>,
}

/// How a drug is priced when dispensed today: its pack, its smallest Teilmenge — or, for a
/// "beliebig teilbar" drug without one, the whole pack priced as a Teilmenge. `None` while the
/// pack has no size or no price.
pub fn pricing(catalogue: &Catalogue, pharmacy: &PharmacyConfig) -> Option<DispensePricing> {
    let pack_size = catalogue.pack_size.filter(|size| *size > Decimal::ZERO)?;
    let pack_price_net = catalogue.pack_price_net?;
    let own = catalogue
        .subset_size
        .zip(catalogue.subset_price_net)
        .map(|(size, price_net)| SubsetPrice { size, price_net });
    let subset = own.or_else(|| {
        if !catalogue.freely_divisible {
            return None;
        }
        money::whole_pack_as_subset(
            catalogue.list_price_net?,
            pack_size,
            catalogue.vat_percent.unwrap_or(Decimal::ZERO),
            pricing_policy(catalogue.human_drug, pharmacy),
        )
    });
    Some(DispensePricing {
        pack_size,
        pack_price_net,
        subset,
        // Without a Teilmenge price there is nothing to bill a rest at: whole packs only.
        freely_divisible: catalogue.freely_divisible && subset.is_some(),
    })
}

/// A drug as it is dispensed today, found from any of its packagings.
#[derive(Debug, Clone)]
pub struct DrugDispense {
    /// The original packaging — what a line references and whose lots it draws from.
    pub original_id: i64,
    pub name: String,
    pub unit: Option<String>,
    pub vat_percent: Decimal,
    pub pricing: DispensePricing,
}

/// Reads how the drug of `packaging_id` is dispensed. Refused as `record.incomplete` while its
/// original packaging is a draft or has no size, price or VAT rate.
pub async fn drug_dispense(
    connection: &mut PgConnection,
    packaging_id: i64,
    pharmacy: &PharmacyConfig,
) -> AppResult<DrugDispense> {
    let row = sqlx::query!(
        r#"SELECT rule.original_id, rule.unit, rule.pack_size, rule.pack_price_net,
                  rule.list_price_net, rule.original_draft, rule.drug_name, rule.vat_percent,
                  rule.human_drug, rule.freely_divisible, rule.subset_size, rule.subset_price_net
           FROM drug_packaging picked
           LEFT JOIN drug_dispense_rule rule ON rule.drug_id = picked.drug_id
           WHERE picked.id = $1"#,
        packaging_id,
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| AppError::field("drug_packaging_id", "record.notFound"))?;

    let incomplete = || AppError::field("drug_packaging_id", "record.incomplete");
    let original_id = row.original_id.ok_or_else(incomplete)?;
    if row.original_draft.unwrap_or(true) {
        return Err(incomplete());
    }
    let vat_percent = row.vat_percent.ok_or_else(incomplete)?;
    let catalogue = Catalogue {
        pack_size: row.pack_size,
        pack_price_net: row.pack_price_net,
        list_price_net: row.list_price_net,
        subset_size: row.subset_size,
        subset_price_net: row.subset_price_net,
        freely_divisible: row.freely_divisible.unwrap_or(false),
        human_drug: row.human_drug.unwrap_or(false),
        vat_percent: Some(vat_percent),
    };
    let pricing = pricing(&catalogue, pharmacy).ok_or_else(incomplete)?;
    Ok(DrugDispense {
        original_id,
        name: row.drug_name.unwrap_or_default(),
        unit: row.unit,
        vat_percent,
        pricing,
    })
}

/// Refuses a quantity the drug cannot be dispensed in, on the field `quantity`.
pub fn check(pricing: &DispensePricing, quantity: Decimal) -> AppResult<()> {
    if quantity <= Decimal::ZERO {
        return Err(AppError::field("quantity", "value.mustBePositive"));
    }
    match money::dispense_check(pricing, quantity) {
        Ok(()) => Ok(()),
        Err(DispenseRefusal::WholePacksOnly) => {
            Err(AppError::field("quantity", "item.wholePacksOnly"))
        }
        Err(DispenseRefusal::SubsetStep) => Err(AppError::field("quantity", "item.subsetStep")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: &str) -> Decimal {
        value.parse().unwrap_or_default()
    }

    fn bottle() -> Catalogue {
        Catalogue {
            pack_size: Some(dec("100")),
            pack_price_net: Some(dec("14.80")),
            list_price_net: Some(dec("10.00")),
            vat_percent: Some(dec("19")),
            ..Catalogue::default()
        }
    }

    #[test]
    fn the_smallest_teilmenge_prices_the_rest() {
        let catalogue = Catalogue {
            subset_size: Some(dec("10")),
            subset_price_net: Some(dec("2.00")),
            ..bottle()
        };
        let priced = pricing(&catalogue, &PharmacyConfig::default());
        let rule = priced.as_ref().map(DispenseRule::from);
        assert_eq!(
            rule,
            Some(DispenseRule {
                pack_size: dec("100"),
                step: Some(dec("10")),
                freely_divisible: false,
                preset: dec("10"),
            })
        );
    }

    #[test]
    fn a_freely_divisible_drug_without_a_teilmenge_takes_its_share_of_the_pack() {
        let catalogue = Catalogue {
            freely_divisible: true,
            ..bottle()
        };
        let priced = pricing(&catalogue, &PharmacyConfig::default());
        assert_eq!(
            priced.and_then(|pricing| pricing.subset),
            Some(SubsetPrice {
                size: dec("100"),
                price_net: dec("20.00"),
            })
        );
        assert_eq!(
            priced
                .as_ref()
                .map(DispenseRule::from)
                .map(|rule| rule.step),
            Some(None)
        );

        // No listed price, no Teilmenge price: whole packs after all.
        let unlisted = Catalogue {
            list_price_net: None,
            ..catalogue
        };
        let priced = pricing(&unlisted, &PharmacyConfig::default());
        assert_eq!(priced.map(|pricing| pricing.freely_divisible), Some(false));
        assert_eq!(
            priced
                .as_ref()
                .map(DispenseRule::from)
                .map(|rule| rule.preset),
            Some(dec("100"))
        );
    }

    #[test]
    fn an_unpriced_pack_cannot_be_dispensed() {
        let catalogue = Catalogue {
            pack_price_net: None,
            ..bottle()
        };
        assert_eq!(pricing(&catalogue, &PharmacyConfig::default()), None);
    }
}
