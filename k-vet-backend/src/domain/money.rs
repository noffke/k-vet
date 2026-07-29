//! Money math — pure functions, unit-tested against worked examples (Constitution II).
//!
//! Grows per story: line totals and VAT extraction (US1), AMPreisV drug prices (US3),
//! GOT § 10 travel expenses (US4).
//!
//! Every amount is a `Decimal`; rounding happens half-up (away from zero) to cents at
//! the points documented next to each formula (research R7).

use rust_decimal::{Decimal, RoundingStrategy};

/// Money is stored and rounded to two decimals.
const CENTS: u32 = 2;

/// Factors and VAT rates are percentages.
fn hundred() -> Decimal {
    Decimal::from(100)
}

/// Rounds to cents, half-up — the rule German invoices use.
pub fn round_money(value: Decimal) -> Decimal {
    value.round_dp_with_strategy(CENTS, RoundingStrategy::MidpointAwayFromZero)
}

/// Gross total of one billing line: `price × quantity × factor/100`, rounded to cents.
///
/// `factor_percent` is the GOT factor (100 = single rate); drug lines pass `None`.
pub fn line_total(
    price_gross: Decimal,
    quantity: Decimal,
    factor_percent: Option<Decimal>,
) -> Decimal {
    let factor = factor_percent.unwrap_or_else(hundred);
    round_money(price_gross * quantity * factor / hundred())
}

/// A gross amount split into net and VAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VatSplit {
    pub gross: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
}

/// Extracts VAT from a gross amount: `net = round(gross / (1 + p/100))`, `vat = gross - net`.
///
/// Deriving VAT as the remainder keeps `net + vat == gross` to the cent, which is what the
/// invoice's VAT summary has to add up to.
pub fn split_vat(gross: Decimal, vat_percent: Decimal) -> VatSplit {
    if vat_percent.is_zero() {
        return VatSplit {
            gross,
            net: gross,
            vat: Decimal::ZERO,
        };
    }
    let net = round_money(gross / (Decimal::ONE + vat_percent / hundred()));
    VatSplit {
        gross,
        net,
        vat: gross - net,
    }
}

/// One VAT rate's share of an invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VatGroup {
    pub vat_percent: Decimal,
    pub gross: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
}

/// Groups `(gross, vat_percent)` line totals per rate, highest rate first.
///
/// VAT is extracted from each group's total, not per line: rounding per line would make
/// the invoice's VAT summary disagree with its own line sums.
pub fn vat_summary(lines: &[(Decimal, Decimal)]) -> Vec<VatGroup> {
    let mut totals: Vec<(Decimal, Decimal)> = Vec::new();
    for (gross, vat_percent) in lines {
        match totals.iter_mut().find(|(rate, _)| rate == vat_percent) {
            Some((_, sum)) => *sum += *gross,
            None => totals.push((*vat_percent, *gross)),
        }
    }
    // Highest rate first — the order German invoices use.
    totals.sort_by_key(|(rate, _)| std::cmp::Reverse(*rate));
    totals
        .into_iter()
        .map(|(vat_percent, gross)| {
            let split = split_vat(gross, vat_percent);
            VatGroup {
                vat_percent,
                gross: split.gross,
                net: split.net,
                vat: split.vat,
            }
        })
        .collect()
}

/// Invoice total — the sum of the group gross amounts.
pub fn total_gross(groups: &[VatGroup]) -> Decimal {
    groups.iter().map(|group| group.gross).sum()
}

// ---------------------------------------------------------------------------
// AMPreisV — the sales price a veterinarian may charge for a drug (research R11)
// ---------------------------------------------------------------------------
//
// Transcribed 2026-07-29 from https://www.gesetze-im-internet.de/ampreisv/
// (Stand: last amended by Art. 2 V v. 9.6.2026 I Nr. 173).
//
// § 10(1): "Bei der Abgabe von Arzneimitteln durch Tierärzte an Tierhalter dürfen
// höchstens Zuschläge entsprechend § 3 Abs. 1 Satz 2 und 3 und Abs. 2 bis 4, § 4 Abs. 1
// und 2 und § 5 Abs. 1 bis 3 sowie die Umsatzsteuer erhoben werden."
// § 10(2): above a basis of 51.13 EUR, the excess carries at most 25 % (to 127.82 EUR)
// and 20 % (beyond).
//
// The basis (§ 3(2)) is the vet's purchase price without VAT — the packaging's list price.

/// Percentage bands of § 3(3): `(upper bound inclusive, percent)`.
const PHARMACY_PERCENT_BANDS: [(&str, &str); 8] = [
    ("1.22", "68"),
    ("3.88", "62"),
    ("7.30", "57"),
    ("12.14", "48"),
    ("19.42", "43"),
    ("29.14", "37"),
    ("543.91", "30"),
    // ab 543,92 Euro: 8,263 Prozent zuzüglich 118,24 Euro — handled separately.
    ("0", "8.263"),
];

/// Fixed amounts of § 3(4) for the gaps between the percentage bands:
/// `(lower bound, upper bound, amount)`, all inclusive.
const PHARMACY_FIXED_BANDS: [(&str, &str, &str); 6] = [
    ("1.23", "1.34", "0.83"),
    ("3.89", "4.22", "2.41"),
    ("7.31", "8.67", "4.16"),
    ("12.15", "13.55", "5.83"),
    ("19.43", "22.57", "8.35"),
    ("29.15", "35.94", "10.78"),
];

/// A computed drug price, so the UI can show what the surcharge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrugPrice {
    /// § 3(2) basis: the purchase price the surcharge is levied on, without VAT.
    pub basis_net: Decimal,
    /// The statutory surcharge, rounded to cents for display.
    pub surcharge: Decimal,
    /// Net sales price, rounded to cents.
    pub net: Decimal,
    /// Gross sales price — the value stored on the packaging.
    pub gross: Decimal,
}

fn decimal(literal: &str) -> Decimal {
    // The literals above are transcribed from the law and parse; a typo would be a bug,
    // so it degrades to zero rather than panicking (Constitution I).
    Decimal::from_str_exact(literal).unwrap_or(Decimal::ZERO)
}

/// § 3(3)/(4): the pharmacy surcharge on a basis, as referenced by § 10(1).
fn pharmacy_surcharge(basis: Decimal) -> Decimal {
    for (from, to, amount) in PHARMACY_FIXED_BANDS {
        if basis >= decimal(from) && basis <= decimal(to) {
            return decimal(amount);
        }
    }
    for (upper, percent) in PHARMACY_PERCENT_BANDS.iter().take(7) {
        if basis <= decimal(upper) {
            return basis * decimal(percent) / hundred();
        }
    }
    // ab 543,92 Euro: 8,263 Prozent zuzüglich 118,24 Euro.
    basis * decimal("8.263") / hundred() + decimal("118.24")
}

/// § 10(1) together with § 10(2): the most a vet may add to the purchase price.
///
/// Up to 51.13 EUR the pharmacy surcharge applies unchanged. Above it, the first
/// 51.13 EUR keep their band's rate and only the excess is surcharged at 25 % / 20 %.
fn vet_surcharge(basis: Decimal) -> Decimal {
    let threshold = decimal("51.13");
    let second_threshold = decimal("127.82");
    if basis <= threshold {
        return pharmacy_surcharge(basis);
    }

    let mut surcharge = pharmacy_surcharge(threshold);
    let middle = basis.min(second_threshold) - threshold;
    surcharge += middle * decimal("25") / hundred();
    if basis > second_threshold {
        surcharge += (basis - second_threshold) * decimal("20") / hundred();
    }
    surcharge
}

/// Sales price of an **original packaging**: purchase price plus the § 3(3)/(4) surcharge
/// capped by § 10(2), plus VAT.
pub fn drug_price_original(list_price_net: Decimal, vat_percent: Decimal) -> DrugPrice {
    price_from(list_price_net, vet_surcharge(list_price_net), vat_percent)
}

/// Sales price of a **subset packaging** (§ 4(1)–(2), the Teilmengenzuschlag).
///
/// The basis is the pro-rata purchase price of the dispensed quantity — the price of the
/// usual pack is decisive — and the surcharge is 100 % (a 50 % margin), plus VAT.
pub fn drug_price_subset(
    original_list_price_net: Decimal,
    original_quantity: Decimal,
    subset_quantity: Decimal,
    vat_percent: Decimal,
) -> DrugPrice {
    if original_quantity <= Decimal::ZERO || subset_quantity <= Decimal::ZERO {
        return price_from(Decimal::ZERO, Decimal::ZERO, vat_percent);
    }
    let basis = original_list_price_net / original_quantity * subset_quantity;
    price_from(basis, basis, vat_percent)
}

/// Pro-rata net purchase price of a subset — what a subset packaging stores as its own
/// list price.
pub fn subset_list_price(
    original_list_price_net: Decimal,
    original_quantity: Decimal,
    subset_quantity: Decimal,
) -> Decimal {
    if original_quantity <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    round_money(original_list_price_net / original_quantity * subset_quantity)
}

fn price_from(basis: Decimal, surcharge: Decimal, vat_percent: Decimal) -> DrugPrice {
    if basis <= Decimal::ZERO {
        return DrugPrice {
            basis_net: Decimal::ZERO,
            surcharge: Decimal::ZERO,
            net: Decimal::ZERO,
            gross: Decimal::ZERO,
        };
    }
    // Rounding happens once, on the gross price that is actually charged; the net and the
    // surcharge are rounded for display only.
    let net = basis + surcharge;
    DrugPrice {
        basis_net: round_money(basis),
        surcharge: round_money(surcharge),
        net: round_money(net),
        gross: round_money(net * (Decimal::ONE + vat_percent / hundred())),
    }
}

// ---------------------------------------------------------------------------
// GOT § 10 Wegegeld — travel expenses (research R10)
// ---------------------------------------------------------------------------

/// The fee schedule allows between one and three times the Wegegeld (§ 10).
const MIN_TRAVEL_MULTIPLIER: &str = "1";
const MAX_TRAVEL_MULTIPLIER: &str = "3";

/// Travel expenses for one trip: `max(km × rate, minimum)`, times the multiplier.
///
/// `km` is the one-way distance the vet enters; the schedule bills per *double*
/// kilometre, so the rate already covers the way back. `multiplier` covers adverse
/// travel conditions and is clamped to the range § 10 permits. Rates come from the
/// operator's configuration because GOT amendments change them.
pub fn travel_expense(
    km: Decimal,
    multiplier: Option<Decimal>,
    rate_per_double_km: Decimal,
    minimum: Decimal,
) -> Decimal {
    let distance = km.max(Decimal::ZERO);
    let base = (distance * rate_per_double_km).max(minimum);
    let multiplier = multiplier.unwrap_or(Decimal::ONE).clamp(
        Decimal::from_str_exact(MIN_TRAVEL_MULTIPLIER).unwrap_or(Decimal::ONE),
        Decimal::from_str_exact(MAX_TRAVEL_MULTIPLIER).unwrap_or(Decimal::ONE),
    );
    round_money(base * multiplier)
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("test literal is a decimal")
    }

    #[test]
    fn line_total_multiplies_price_quantity_and_factor() {
        // Two packages at 12.50 EUR.
        assert_eq!(line_total(dec("12.50"), dec("2"), None), dec("25.00"));
        // A GOT position at the 1.5-fold rate.
        assert_eq!(
            line_total(dec("23.62"), dec("1"), Some(dec("150"))),
            dec("35.43")
        );
        // Fractional quantities (30 ml of a liquid).
        assert_eq!(line_total(dec("0.50"), dec("30"), None), dec("15.00"));
        // The default factor of 100 % changes nothing.
        assert_eq!(
            line_total(dec("10.00"), dec("1"), Some(dec("100"))),
            dec("10.00")
        );
    }

    #[test]
    fn line_total_rounds_half_up_to_cents() {
        // 0.05 × 2.5 = 0.125 → 0.13
        assert_eq!(line_total(dec("0.05"), dec("2.5"), None), dec("0.13"));
        // 23.62 × 2.3 = 54.326 → 54.33
        assert_eq!(
            line_total(dec("23.62"), dec("1"), Some(dec("230"))),
            dec("54.33")
        );
        // 0.015 → 0.02 (away from zero, not banker's rounding)
        assert_eq!(line_total(dec("0.01"), dec("1.5"), None), dec("0.02"));
    }

    #[test]
    fn vat_is_extracted_from_the_gross_amount() {
        let normal = split_vat(dec("119.00"), dec("19"));
        assert_eq!(normal.gross, dec("119.00"));
        assert_eq!(normal.net, dec("100.00"));
        assert_eq!(normal.vat, dec("19.00"));

        let reduced = split_vat(dec("107.00"), dec("7"));
        assert_eq!(reduced.net, dec("100.00"));
        assert_eq!(reduced.vat, dec("7.00"));
    }

    #[test]
    fn vat_extraction_rounds_and_stays_consistent() {
        // 35.43 / 1.19 = 29.7731… → net 29.77, and net + vat must equal gross exactly.
        let split = split_vat(dec("35.43"), dec("19"));
        assert_eq!(split.net, dec("29.77"));
        assert_eq!(split.vat, dec("5.66"));
        assert_eq!(split.net + split.vat, split.gross);
    }

    #[test]
    fn zero_vat_leaves_the_amount_untouched() {
        let split = split_vat(dec("42.00"), Decimal::ZERO);
        assert_eq!(split.net, dec("42.00"));
        assert_eq!(split.vat, Decimal::ZERO);
    }

    #[test]
    fn vat_summary_groups_lines_by_rate() {
        let groups = vat_summary(&[
            (dec("119.00"), dec("19")),
            (dec("11.90"), dec("19")),
            (dec("107.00"), dec("7")),
        ]);

        assert_eq!(groups.len(), 2, "one group per rate");
        // Highest rate first — the order German invoices use.
        assert_eq!(groups[0].vat_percent, dec("19"));
        assert_eq!(groups[0].gross, dec("130.90"));
        assert_eq!(groups[0].net, dec("110.00"));
        assert_eq!(groups[0].vat, dec("20.90"));
        assert_eq!(groups[1].vat_percent, dec("7"));
        assert_eq!(groups[1].vat, dec("7.00"));
    }

    #[test]
    fn vat_summary_is_computed_from_the_group_total_not_per_line() {
        // Two lines of 0.05 EUR: per-line rounding would give 0.01 + 0.01 = 0.02,
        // the correct answer for the invoice is VAT on 0.10.
        let groups = vat_summary(&[(dec("0.05"), dec("19")), (dec("0.05"), dec("19"))]);
        assert_eq!(groups[0].gross, dec("0.10"));
        assert_eq!(groups[0].vat, dec("0.02"));
        assert_eq!(groups[0].net, dec("0.08"));
    }

    #[test]
    fn total_sums_the_group_gross_amounts() {
        let groups = vat_summary(&[(dec("119.00"), dec("19")), (dec("107.00"), dec("7"))]);
        assert_eq!(total_gross(&groups), dec("226.00"));
    }

    // ── AMPreisV (T049) ──────────────────────────────────────────────────────────
    //
    // Transcribed 2026-07-29 from https://www.gesetze-im-internet.de/ampreisv/
    // (Stand: last amended by Art. 2 V v. 9.6.2026). A veterinarian dispensing to an
    // animal owner may charge at most the surcharges of § 3(1) S.2–3 and § 3(2)–(4),
    // § 4(1)–(2) and § 5(1)–(3), plus VAT (§ 10(1)); above a basis of 51.13 EUR the
    // reduced rates of § 10(2) apply to the excess.
    //
    // ⚠ FOR VET REVIEW (SC-005): the worked examples below are the reference values the
    // practice bills on. Each one names the paragraph it exercises.

    #[test]
    fn original_packaging_uses_the_percentage_tiers_of_ampreisv_3_3() {
        // § 3(3): bis 1,22 Euro → 68 %. 1.00 + 0.68 = 1.68 net, ×1.19 = 2.00 gross.
        let cheap = drug_price_original(dec("1.00"), dec("19"));
        assert_eq!(cheap.surcharge, dec("0.68"));
        assert_eq!(cheap.net, dec("1.68"));
        assert_eq!(cheap.gross, dec("2.00"));

        // § 3(3): von 8,68 bis 12,14 Euro → 48 %. 10.00 + 4.80 = 14.80 net → 17.61 gross.
        let middle = drug_price_original(dec("10.00"), dec("19"));
        assert_eq!(middle.surcharge, dec("4.80"));
        assert_eq!(middle.gross, dec("17.61"));

        // § 3(3): von 35,95 bis 543,91 Euro → 30 %. 40.00 + 12.00 = 52.00 → 61.88 gross.
        let upper = drug_price_original(dec("40.00"), dec("19"));
        assert_eq!(upper.surcharge, dec("12.00"));
        assert_eq!(upper.gross, dec("61.88"));
    }

    #[test]
    fn the_fixed_amounts_of_ampreisv_3_4_fill_the_gaps_between_the_tiers() {
        // § 3(4): von 1,23 bis 1,34 Euro → 0,83 Euro.
        let first_gap = drug_price_original(dec("1.30"), dec("19"));
        assert_eq!(first_gap.surcharge, dec("0.83"));
        assert_eq!(first_gap.gross, dec("2.53"));

        // § 3(4): von 19,43 bis 22,57 Euro → 8,35 Euro.
        let fifth_gap = drug_price_original(dec("20.00"), dec("19"));
        assert_eq!(fifth_gap.surcharge, dec("8.35"));
        assert_eq!(fifth_gap.gross, dec("33.74"));

        // § 3(4): von 29,15 bis 35,94 Euro → 10,78 Euro.
        let last_gap = drug_price_original(dec("30.00"), dec("19"));
        assert_eq!(last_gap.surcharge, dec("10.78"));
    }

    #[test]
    fn every_tier_boundary_is_covered_exactly_once() {
        // The bands of § 3(3) and § 3(4) partition the range; walking the boundaries must
        // never produce a gap (a missing band would silently price a drug at zero margin).
        for cents in 1..60_000u64 {
            let basis = Decimal::new(i64::try_from(cents).unwrap_or(i64::MAX), 2);
            let price = drug_price_original(basis, Decimal::ZERO);
            assert!(
                price.surcharge > Decimal::ZERO,
                "no surcharge band covers a basis of {basis}"
            );
            assert!(
                price.net > basis,
                "the net price must exceed the purchase price"
            );
        }
    }

    #[test]
    fn expensive_drugs_fall_under_the_reduced_rates_of_ampreisv_10_2() {
        // § 10(2): 30 % on the first 51.13 (§ 3(3) band) plus 25 % of the excess up to
        // 127.82. 15.339 + 12.2175 = 27.5565 → net 127.5565 → gross 151.79.
        let hundred = drug_price_original(dec("100.00"), dec("19"));
        assert_eq!(hundred.surcharge, dec("27.56"));
        assert_eq!(hundred.gross, dec("151.79"));

        // § 10(2): plus 20 % of the part above 127.82.
        // 15.339 + 19.1725 + 14.436 = 48.9475 → net 248.9475 → gross 296.25.
        let expensive = drug_price_original(dec("200.00"), dec("19"));
        assert_eq!(expensive.surcharge, dec("48.95"));
        assert_eq!(expensive.gross, dec("296.25"));

        // The vet's margin shrinks as the drug gets dearer — that is the point of § 10(2).
        assert!(
            expensive.surcharge / dec("200") < hundred.surcharge / dec("100"),
            "the relative surcharge must fall"
        );
    }

    #[test]
    fn the_surcharge_never_jumps_backwards_across_a_band() {
        // A cent more purchase price must never mean a cent less sales price.
        let mut previous = Decimal::ZERO;
        for cents in 1..20_000u64 {
            let basis = Decimal::new(i64::try_from(cents).unwrap_or(i64::MAX), 2);
            let net = drug_price_original(basis, Decimal::ZERO).net;
            assert!(net >= previous, "the net price fell at a basis of {basis}");
            previous = net;
        }
    }

    #[test]
    fn subset_packagings_carry_the_teilmengenzuschlag_of_ampreisv_4() {
        // § 4(1)–(2): the basis is the pro-rata purchase price of the dispensed quantity
        // (the usual pack's price is decisive), the surcharge is 100 % (margin 50 %).
        // 10 ml out of a 100 ml bottle bought for 10.00 → basis 1.00 → net 2.00 → 2.38.
        let subset = drug_price_subset(dec("10.00"), dec("100"), dec("10"), dec("19"));
        assert_eq!(subset.basis_net, dec("1.00"));
        assert_eq!(subset.surcharge, dec("1.00"));
        assert_eq!(subset.net, dec("2.00"));
        assert_eq!(subset.gross, dec("2.38"));

        // A half pack: basis 5.00 → net 10.00 → 11.90 gross.
        let half = drug_price_subset(dec("10.00"), dec("100"), dec("50"), dec("19"));
        assert_eq!(half.net, dec("10.00"));
        assert_eq!(half.gross, dec("11.90"));

        // The reduced VAT rate is applied just as faithfully.
        let reduced = drug_price_subset(dec("10.00"), dec("100"), dec("10"), dec("7"));
        assert_eq!(reduced.gross, dec("2.14"));
    }

    #[test]
    fn a_subset_of_the_whole_pack_costs_more_than_the_pack_itself() {
        // Dispensing 100 ml as a "subset" is priced per § 4 (100 %), the whole bottle per
        // § 3(3) (48 % in this band) — the surcharge for repackaging is the difference.
        let pack = drug_price_original(dec("10.00"), dec("19"));
        let all_of_it = drug_price_subset(dec("10.00"), dec("100"), dec("100"), dec("19"));
        assert!(
            all_of_it.gross > pack.gross,
            "{} vs {}",
            all_of_it.gross,
            pack.gross
        );
    }

    #[test]
    fn a_zero_or_negative_purchase_price_yields_no_price() {
        assert_eq!(
            drug_price_original(Decimal::ZERO, dec("19")).gross,
            Decimal::ZERO
        );
        assert_eq!(
            drug_price_subset(dec("10.00"), dec("100"), Decimal::ZERO, dec("19")).gross,
            Decimal::ZERO
        );
        // A packaging without a quantity cannot be priced pro rata.
        assert_eq!(
            drug_price_subset(dec("10.00"), Decimal::ZERO, dec("10"), dec("19")).gross,
            Decimal::ZERO
        );
    }

    // ── GOT § 10 Wegegeld (T058) ─────────────────────────────────────────────────
    //
    // GOT 2022 § 10(2): "je Doppelkilometer 3,50 Euro, insgesamt jedoch mindestens
    // 13 Euro" (research R10). The rates live in the config because GOT amendments change
    // them; the ×1–×3 multiplier covers the "widrige Verkehrsverhältnisse" clause.
    //
    // ⚠ FOR VET REVIEW (SC-005).

    fn rates() -> (Decimal, Decimal) {
        (dec("3.50"), dec("13.00"))
    }

    #[test]
    fn travel_expenses_are_charged_per_double_kilometre() {
        let (rate, minimum) = rates();
        // 10 km × 3.50 = 35.00.
        assert_eq!(travel_expense(dec("10"), None, rate, minimum), dec("35.00"));
        // 27 km × 3.50 = 94.50.
        assert_eq!(travel_expense(dec("27"), None, rate, minimum), dec("94.50"));
        // Fractional distances are allowed (a pro-rated shared trip).
        assert_eq!(
            travel_expense(dec("7.5"), None, rate, minimum),
            dec("26.25")
        );
    }

    #[test]
    fn the_legal_minimum_applies_to_short_distances() {
        let (rate, minimum) = rates();
        // 2 km × 3.50 = 7.00 → below the 13.00 minimum.
        assert_eq!(travel_expense(dec("2"), None, rate, minimum), dec("13.00"));
        // The threshold sits at 13 / 3.50 = 3.714… km.
        assert_eq!(
            travel_expense(dec("3.71"), None, rate, minimum),
            dec("13.00")
        );
        assert_eq!(travel_expense(dec("4"), None, rate, minimum), dec("14.00"));
        // A visit next door still costs the minimum.
        assert_eq!(
            travel_expense(Decimal::ZERO, None, rate, minimum),
            dec("13.00")
        );
    }

    #[test]
    fn the_multiplier_covers_adverse_travel_conditions() {
        let (rate, minimum) = rates();
        // Twice the Wegegeld for 10 km.
        assert_eq!(
            travel_expense(dec("10"), Some(dec("2")), rate, minimum),
            dec("70.00")
        );
        // The multiplier applies to the minimum as well.
        assert_eq!(
            travel_expense(dec("2"), Some(dec("3")), rate, minimum),
            dec("39.00")
        );
        // A missing multiplier is the single rate.
        assert_eq!(
            travel_expense(dec("10"), Some(Decimal::ONE), rate, minimum),
            travel_expense(dec("10"), None, rate, minimum)
        );
    }

    #[test]
    fn the_multiplier_stays_within_the_range_the_fee_schedule_allows() {
        let (rate, minimum) = rates();
        // § 10 allows up to three times; anything beyond is clamped, not honoured.
        assert_eq!(
            travel_expense(dec("10"), Some(dec("5")), rate, minimum),
            dec("105.00")
        );
        // Below one it would reduce the statutory fee.
        assert_eq!(
            travel_expense(dec("10"), Some(dec("0.5")), rate, minimum),
            dec("35.00")
        );
    }

    #[test]
    fn changed_rates_flow_straight_through() {
        // A GOT amendment is a config change, not a code change.
        assert_eq!(
            travel_expense(dec("10"), None, dec("4.00"), dec("15.00")),
            dec("40.00")
        );
        assert_eq!(
            travel_expense(dec("2"), None, dec("4.00"), dec("15.00")),
            dec("15.00")
        );
    }
}
