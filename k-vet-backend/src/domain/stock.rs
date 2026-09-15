//! Stock ledger: FEFO allocation, draft dispenses and compensating corrections.
//!
//! Remaining stock is never stored (FR-018) — it is `initial_quantity + SUM(movements)`,
//! read through the `lot_remaining` view. A dispense stays a *draft* while the treatment
//! has no accepted invoice: draft dispenses are deleted and rewritten freely as the vet
//! edits lines. Once the invoice is accepted the ledger is append-only, and cancelling
//! writes linked counter-movements instead of deleting anything ("Storno statt Löschen").

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{PgConnection, PgExecutor};

use crate::error::{AppError, AppResult};

/// What a lot still holds, for FEFO ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotAvailability {
    pub lot_id: i64,
    pub remaining: Decimal,
    pub expiration_date: Option<NaiveDate>,
}

/// One lot's share of a dispense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LotAllocation {
    pub lot_id: i64,
    pub quantity: Decimal,
}

/// How much of a dispense the lots could not cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortfall {
    pub needed: Decimal,
    pub available: Decimal,
}

/// Splits `needed` across lots, earliest expiration first (FR-019).
///
/// `lots` must already be ordered FEFO (expiration date ascending, undated last). When they
/// cannot cover the quantity the dispense is refused with what is missing, rather than booked
/// against a lot that cannot supply it (FR-018, issues.md 8).
///
/// This used to add the shortfall to the first lot and let its derived stock go negative, on
/// the reasoning that the physical shelf is the truth. A negative remainder is not the shelf,
/// though — it says the books were already wrong, and it spreads quietly, because FEFO goes on
/// offering lots that hold nothing. The remedy is the stocktake correction, entered against
/// what is actually on the shelf; refusing here is what sends the vet to it, at the moment the
/// discrepancy shows up and can still be counted.
pub fn allocate_fefo(
    lots: &[LotAvailability],
    needed: Decimal,
) -> Result<Vec<LotAllocation>, Shortfall> {
    if needed <= Decimal::ZERO {
        return Ok(Vec::new());
    }
    let mut allocations: Vec<LotAllocation> = Vec::new();
    let mut rest = needed;

    for lot in lots {
        if rest <= Decimal::ZERO {
            break;
        }
        if lot.remaining <= Decimal::ZERO {
            continue;
        }
        let take = rest.min(lot.remaining);
        allocations.push(LotAllocation {
            lot_id: lot.lot_id,
            quantity: take,
        });
        rest -= take;
    }

    if rest > Decimal::ZERO {
        return Err(Shortfall {
            needed,
            available: needed - rest,
        });
    }
    Ok(allocations)
}

/// Lots of a packaging in FEFO order, with their derived remaining quantity.
pub async fn lot_availability<'e, E>(
    executor: E,
    packaging_id: i64,
) -> AppResult<Vec<LotAvailability>>
where
    E: PgExecutor<'e>,
{
    let rows = sqlx::query!(
        r#"SELECT lot_id, remaining, expiration_date
           FROM lot_remaining
           WHERE packaging_id = $1
           ORDER BY expiration_date ASC NULLS LAST, lot_id ASC"#,
        packaging_id,
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LotAvailability {
            lot_id: row.lot_id.unwrap_or_default(),
            remaining: row.remaining.unwrap_or_default(),
            expiration_date: row.expiration_date,
        })
        .collect())
}

/// Total derived stock over all lots of a packaging.
pub async fn total_available<'e, E>(executor: E, packaging_id: i64) -> AppResult<Decimal>
where
    E: PgExecutor<'e>,
{
    let total: Option<Decimal> = sqlx::query_scalar!(
        "SELECT COALESCE(SUM(remaining), 0) FROM lot_remaining WHERE packaging_id = $1",
        packaging_id,
    )
    .fetch_one(executor)
    .await?;
    Ok(total.unwrap_or_default())
}

/// `true` once the treatment's invoice is released — from then on the dispense movements are
/// frozen and the ledger is append-only (FR-020). Acceptance is the freeze point; sending and
/// handing over are later stages of the same released invoice.
pub async fn treatment_is_frozen<'e, E>(executor: E, treatment_id: i64) -> AppResult<bool>
where
    E: PgExecutor<'e>,
{
    let frozen: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT 1 FROM invoice
             WHERE treatment_id = $1 AND status IN ('accepted', 'sent', 'submitted')
         )",
        treatment_id,
    )
    .fetch_one(executor)
    .await?;
    Ok(frozen.unwrap_or(false))
}

/// Rejects edits to a treatment whose stock movements are already frozen.
pub async fn ensure_editable(connection: &mut PgConnection, treatment_id: i64) -> AppResult<()> {
    if treatment_is_frozen(&mut *connection, treatment_id).await? {
        return Err(AppError::Conflict(
            "the treatment's invoice is accepted — update the invoice to edit it".to_owned(),
        ));
    }
    Ok(())
}

/// Where a dispense is booked, and how much stock one billed packaging consumes.
///
/// Lots only ever belong to a drug's **original** packaging, so dispensing a subset
/// packaging draws from the original's lots: three 10 ml subsets take 30 ml off the shelf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockTarget {
    /// The original packaging whose lots carry the stock.
    pub packaging_id: i64,
    /// Base units (e.g. ml) consumed per one unit of the billed packaging.
    pub base_units: Decimal,
}

/// Resolves the stock target for a billed packaging. `None` when the drug has no original
/// packaging yet — nothing can be booked then.
pub async fn stock_target<'e, E>(executor: E, packaging_id: i64) -> AppResult<Option<StockTarget>>
where
    E: PgExecutor<'e>,
{
    let row = sqlx::query!(
        r#"SELECT original.id AS "original_id?", billed.quantity AS "billed_quantity?"
           FROM drug_packaging billed
           LEFT JOIN drug_packaging original
                  ON original.drug_id = billed.drug_id AND original.kind = 'original'
           WHERE billed.id = $1"#,
        packaging_id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.and_then(|row| {
        Some(StockTarget {
            packaging_id: row.original_id?,
            base_units: row.billed_quantity.unwrap_or(Decimal::ONE),
        })
    }))
}

/// Replaces the draft dispenses of a drug line with a fresh FEFO allocation.
///
/// `billed_quantity` counts packagings; the booked amount is converted to base units.
/// Returns the allocation actually booked, so the API can tell the UI which lots were
/// used and whether stock was short.
pub async fn rewrite_draft_dispenses(
    connection: &mut PgConnection,
    treatment_item_id: i64,
    packaging_id: i64,
    billed_quantity: Decimal,
) -> AppResult<Vec<LotAllocation>> {
    let Some(target) = stock_target(&mut *connection, packaging_id).await? else {
        tracing::warn!(
            packaging_id,
            "no original packaging for this drug — no stock movement booked"
        );
        return Ok(Vec::new());
    };
    let needed = billed_quantity * target.base_units;
    let lots = lot_availability(&mut *connection, target.packaging_id).await?;
    // Refused rather than booked against a lot that cannot supply it. The message points at
    // the stocktake, because that is the thing the vet has to do next — the numbers themselves
    // are on the lot page she is being sent to, and field errors carry no interpolation.
    let allocations = allocate_fefo(&lots, needed).map_err(|short| {
        tracing::info!(
            needed = %short.needed,
            available = %short.available,
            packaging_id = target.packaging_id,
            "dispense refused: the lots cannot cover it",
        );
        AppError::field("quantity", "item.stockShort")
    })?;
    write_dispenses(connection, treatment_item_id, &allocations).await?;
    Ok(allocations)
}

/// Replaces the draft dispenses with an explicit, user-chosen lot selection (FR-019).
pub async fn rewrite_draft_dispenses_with_lots(
    connection: &mut PgConnection,
    treatment_item_id: i64,
    allocations: &[LotAllocation],
) -> AppResult<()> {
    write_dispenses(connection, treatment_item_id, allocations).await
}

/// Deletes the *draft* dispenses of a line (line removed, quantity changed).
///
/// A dispense that already carries a compensating correction was frozen once and is
/// history — it is never deleted. After an accept/cancel cycle a fresh edit therefore adds
/// new movements on top of the reversed pair instead of rewriting the past.
pub async fn remove_draft_dispenses(
    connection: &mut PgConnection,
    treatment_item_id: i64,
) -> AppResult<()> {
    sqlx::query!(
        "DELETE FROM drug_stock_movement movement
         WHERE movement.treatment_item_id = $1
           AND movement.kind = 'dispense'
           AND NOT EXISTS (
               SELECT 1 FROM drug_stock_movement reversal
               WHERE reversal.reverses_movement_id = movement.id
           )",
        treatment_item_id,
    )
    .execute(connection)
    .await?;
    Ok(())
}

async fn write_dispenses(
    connection: &mut PgConnection,
    treatment_item_id: i64,
    allocations: &[LotAllocation],
) -> AppResult<()> {
    remove_draft_dispenses(&mut *connection, treatment_item_id).await?;
    for allocation in allocations {
        if allocation.quantity <= Decimal::ZERO {
            continue;
        }
        sqlx::query!(
            "INSERT INTO drug_stock_movement (lot_id, kind, quantity, treatment_item_id)
             VALUES ($1, 'dispense', $2, $3)",
            allocation.lot_id,
            -allocation.quantity,
            treatment_item_id,
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

/// Writes compensating corrections for every frozen dispense of a treatment, each linked
/// to the movement it reverses. Idempotent: an already reversed movement is skipped.
pub async fn reverse_treatment_dispenses(
    connection: &mut PgConnection,
    treatment_id: i64,
) -> AppResult<u64> {
    let movements = sqlx::query!(
        r#"SELECT movement.id, movement.lot_id, movement.quantity
           FROM drug_stock_movement movement
           JOIN treatment_item item ON item.id = movement.treatment_item_id
           WHERE item.treatment_id = $1
             AND movement.kind = 'dispense'
             AND NOT EXISTS (
                 SELECT 1 FROM drug_stock_movement reversal
                 WHERE reversal.reverses_movement_id = movement.id
             )"#,
        treatment_id,
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut written = 0u64;
    for movement in movements {
        sqlx::query!(
            "INSERT INTO drug_stock_movement
                 (lot_id, kind, quantity, reason, reverses_movement_id)
             VALUES ($1, 'correction', $2, $3, $4)",
            movement.lot_id,
            -movement.quantity,
            "Rechnungsstorno",
            movement.id,
        )
        .execute(&mut *connection)
        .await?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("test literal is a decimal")
    }

    fn date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, month, day)
    }

    fn lots() -> Vec<LotAvailability> {
        vec![
            LotAvailability {
                lot_id: 1,
                remaining: dec("20"),
                expiration_date: date(2026, 6, 30),
            },
            LotAvailability {
                lot_id: 2,
                remaining: dec("100"),
                expiration_date: date(2027, 1, 31),
            },
            LotAvailability {
                lot_id: 3,
                remaining: dec("50"),
                expiration_date: None,
            },
        ]
    }

    #[test]
    fn takes_the_lot_expiring_first() {
        let allocations = allocate_fefo(&lots(), dec("5")).expect("one lot covers it");
        assert_eq!(
            allocations,
            vec![LotAllocation {
                lot_id: 1,
                quantity: dec("5")
            }]
        );
    }

    #[test]
    fn splits_across_lots_when_the_first_is_too_small() {
        let allocations = allocate_fefo(&lots(), dec("30")).expect("two lots cover it");
        assert_eq!(
            allocations,
            vec![
                LotAllocation {
                    lot_id: 1,
                    quantity: dec("20")
                },
                LotAllocation {
                    lot_id: 2,
                    quantity: dec("10")
                },
            ]
        );
    }

    #[test]
    fn undated_lots_are_used_last() {
        let allocations = allocate_fefo(&lots(), dec("130")).expect("three lots cover it");
        assert_eq!(
            allocations,
            vec![
                LotAllocation {
                    lot_id: 1,
                    quantity: dec("20")
                },
                LotAllocation {
                    lot_id: 2,
                    quantity: dec("100")
                },
                LotAllocation {
                    lot_id: 3,
                    quantity: dec("10")
                },
            ]
        );
    }

    /// Reversed deliberately (FR-018, issues.md 8): this used to assert that the full dispense
    /// was always recorded and the lot allowed to go negative.
    #[test]
    fn a_dispense_the_lots_cannot_cover_is_refused() {
        // 200 needed, 170 on the books.
        let short = allocate_fefo(&lots(), dec("200")).expect_err("more than the lots hold");
        assert_eq!(short.needed, dec("200"));
        assert_eq!(
            short.available,
            dec("170"),
            "how much there is, so the vet knows what to count",
        );
    }

    #[test]
    fn dispensing_without_any_stock_is_refused_too() {
        let empty = vec![LotAvailability {
            lot_id: 9,
            remaining: Decimal::ZERO,
            expiration_date: None,
        }];
        let short = allocate_fefo(&empty, dec("7")).expect_err("an empty lot supplies nothing");
        assert_eq!(short.available, Decimal::ZERO);
    }

    /// Exactly the stock there is has to go through — the boundary is where an off-by-one in
    /// the comparison would show up, and a refusal here would stop the vet dispensing the last
    /// of a bottle.
    #[test]
    fn the_last_of_the_stock_can_still_be_dispensed() {
        let allocations = allocate_fefo(&lots(), dec("170")).expect("exactly what is there");
        let booked: Decimal = allocations
            .iter()
            .map(|allocation| allocation.quantity)
            .sum();
        assert_eq!(booked, dec("170"));
    }

    #[test]
    fn nothing_is_allocated_without_lots_or_quantity() {
        assert!(
            allocate_fefo(&lots(), Decimal::ZERO)
                .expect("nothing needed, nothing taken")
                .is_empty()
        );
        // No lots at all and a quantity wanted is a shortfall like any other.
        assert!(allocate_fefo(&[], dec("5")).is_err());
    }
}
