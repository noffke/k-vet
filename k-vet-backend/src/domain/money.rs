//! Money math — pure functions, unit-tested against worked examples (Constitution II).
//!
//! Grows per story: line totals and VAT (US1), AMPreisV drug prices (US3), GOT § 10 travel
//! expenses (US4).
//!
//! **Prices are net.** The GOT publishes net fees, AMPreisV § 3(2) levies its surcharges on a
//! net listed price, and § 14 UStG states an invoice as Entgelt plus Steuerbetrag — so the net
//! is the figure the law computes and the catalogue stores, and the gross is derived from it.
//! (This was the other way round until migration `0009`, which meant every GOT position was
//! billed roughly 16 % too low; see that migration for the full story.)
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

/// Net total of one billing line: `price × quantity × factor/100`, rounded to cents.
///
/// Prices are **net** throughout (see the module header); the gross follows from
/// [`add_vat`]. `factor_percent` is the GOT factor (100 = single rate); drug lines pass
/// `None`.
pub fn line_total(
    price_net: Decimal,
    quantity: Decimal,
    factor_percent: Option<Decimal>,
) -> Decimal {
    let factor = factor_percent.unwrap_or_else(hundred);
    round_money(price_net * quantity * factor / hundred())
}

/// A net amount together with its VAT and the resulting gross.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VatSplit {
    pub gross: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
}

/// Adds VAT to a net amount: `vat = round(net × p/100)`, `gross = net + vat`.
///
/// This is the direction § 14 UStG states an invoice in — the Steuerbetrag is computed *from*
/// the Entgelt, not extracted out of a gross figure — and it is the direction the fee schedules
/// compute in (GOT publishes net fees, AMPreisV § 3(2) levies its surcharges on a net listed
/// price). Taking the gross as the sum keeps `net + vat == gross` exact to the cent.
pub fn add_vat(net: Decimal, vat_percent: Decimal) -> VatSplit {
    if vat_percent.is_zero() {
        return VatSplit {
            gross: net,
            net,
            vat: Decimal::ZERO,
        };
    }
    let vat = round_money(net * vat_percent / hundred());
    VatSplit {
        gross: net + vat,
        net,
        vat,
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

/// Groups `(net, vat_percent)` line totals per rate, highest rate first.
///
/// VAT is rounded **per line** and then summed — the *horizontale Berechnung*, one of the two
/// methods German practice accepts under § 14 UStG (the other sums the raw amounts and rounds the
/// total). It is the right one here because the invoice prints a gross column: with the VAT of a
/// line rounded first, that line's gross is `net + vat` exactly, and the column therefore adds up
/// to the group's gross by construction. Rounding on the group total instead leaves the printed
/// column a cent adrift — visible on the invoice this replaces, whose GOT 40 line shows 41,05 €
/// per unit against a 41,06 € line total.
///
/// The cost is inherent to the method and accepted: a group's VAT is the sum of its lines' VAT
/// rather than the rate applied to the group's net, so the two can differ by a cent or so on a
/// long invoice. § 14 Abs. 4 UStG asks for the Entgelt per rate (Nr. 7) and the Steuerbetrag on it
/// (Nr. 8); it does not prescribe which of the two roundings produces them.
pub fn vat_summary(lines: &[(Decimal, Decimal)]) -> Vec<VatGroup> {
    let mut totals: Vec<VatGroup> = Vec::new();
    for (net, vat_percent) in lines {
        let line = add_vat(*net, *vat_percent);
        match totals
            .iter_mut()
            .find(|group| group.vat_percent == *vat_percent)
        {
            Some(group) => {
                group.net += line.net;
                group.vat += line.vat;
                group.gross += line.gross;
            }
            None => totals.push(VatGroup {
                vat_percent: *vat_percent,
                net: line.net,
                vat: line.vat,
                gross: line.gross,
            }),
        }
    }
    // Highest rate first — the order German invoices use.
    totals.sort_by_key(|group| std::cmp::Reverse(group.vat_percent));
    totals
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
// § 10(2): "Liegt der für den Zuschlag entsprechend § 3 Abs. 2 maßgebliche Betrag über
// 51,13 Euro, so sind für den 51,13 Euro übersteigenden Betrag folgende Zuschläge zu erheben:
// von 51,13 Euro bis 127,82 Euro höchstens 25 Prozent, von mehr als 127,82 Euro höchstens
// 20 Prozent."
//
// The basis (§ 3(2)) is a **listed** price without VAT: the manufacturer's Abgabepreis plus the
// § 2 wholesale surcharge — the figure a wholesaler's catalogue quotes. It is *not* what the
// practice negotiated; a rebate does not lower what may be charged on. Note that price lists,
// Barsoi among them, label this figure "Einkaufspreis": in AMPreisV usage that word already
// means the listed purchase price, which is why the column is called `list_price_net`. Same
// number, unambiguous name.

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

/// Which Satz of § 3 Abs. 1 a drug falls under when a vet dispenses it.
///
/// § 10 Abs. 1 permits surcharges "entsprechend § 3 Abs. 1 Satz 2 und 3" — one Satz per case, so
/// the choice is a property of the drug, not of the practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrugRule {
    /// § 3 Abs. 1 Satz 3 → the bands of Abs. 3/4: a medicine approved for animals.
    #[default]
    Veterinary,
    /// § 3 Abs. 1 Satz 2: a human medicine dispensed for use in an animal ("Umwidmung").
    Human,
}

/// How this installation prices drugs: the statutory rule, plus one deliberate deviation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PricingPolicy {
    pub rule: DrugRule,
    /// Never price a Teilmenge below its share of the whole pack — see [`drug_price_subset`].
    pub subset_proportional_floor: bool,
}

impl PricingPolicy {
    /// The statutory default: veterinary rule, no floor.
    pub fn veterinary() -> Self {
        Self::default()
    }
}

/// A computed drug price, so the UI can show what the surcharge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrugPrice {
    /// § 3(2) basis: the listed price the surcharge is levied on, without VAT.
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

/// § 10(1) together with § 10(2): the most a vet may add to the listed price.
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

/// § 3 Abs. 1 Satz 2: the most a vet may add to a **human** medicine used on an animal.
///
/// "höchstens ein Zuschlag von 3 Prozent zuzüglich 8,10 Euro". Flat where the veterinary bands are
/// proportional, so it is far more than the bands allow on a cheap drug and far less on a dear one;
/// the two cross somewhere around 20–30 EUR of listed price.
///
/// § 10(2) is deliberately not applied on top: it caps the surcharge on the part above 51,13 EUR at
/// 25 % and then 20 %, and 3 % never reaches either, so it cannot bind. A test pins that down
/// rather than leaving it to be re-derived.
fn human_surcharge(basis: Decimal) -> Decimal {
    basis * decimal("3") / hundred() + decimal("8.10")
}

/// Sales price of an **original packaging**: the listed price plus the statutory surcharge for its
/// rule — § 3(3)/(4) capped by § 10(2) for a veterinary medicine, § 3 Abs. 1 Satz 2 for a human
/// one — plus VAT.
pub fn drug_price_original(
    list_price_net: Decimal,
    vat_percent: Decimal,
    policy: PricingPolicy,
) -> DrugPrice {
    let surcharge = match policy.rule {
        DrugRule::Veterinary => vet_surcharge(list_price_net),
        DrugRule::Human => human_surcharge(list_price_net),
    };
    price_from(list_price_net, surcharge, vat_percent)
}

/// Sales price of a **subset packaging** — the Teilmengenzuschlag, *in Anlehnung an* § 4(1)–(2).
///
/// The basis is the pro-rata listed price of the dispensed quantity — "der Einkaufspreis der
/// üblichen Abpackung ist maßgebend" (§ 4 Abs. 2) — and the surcharge is 100 % (a 50 % margin),
/// plus VAT.
///
/// **§ 4 does not literally cover this case.** It speaks of a *Stoff*, which AMG § 3 defines as a
/// chemical element or compound, a plant, animal material or a microorganism — table salt, not a
/// pack of Carprofen. A part-pack of a finished medicine is a Fertigarzneimittel (AMG § 4 Abs. 1).
/// The Bundestierärztekammer put it to the BMG on 2018-12-14 that "die Preisberechnung für aus
/// Fertigarzneimitteln entnommene Teilmengen erfolgt derzeit in Anlehnung an § 4 AMPreisV", and
/// that the regulation "ist in diesem Punkt lückenhaft". § 10 Abs. 1's "entsprechend" is what
/// carries the analogy. So this is settled practice on an acknowledged gap, not a rule read off
/// the page — worth knowing before anyone rewrites it from the statute alone.
///
/// # The proportional floor
///
/// For a **human** preparation the whole pack carries a flat 8,10 EUR while a Teilmenge carries
/// only a percentage, so a cheap one comes out *below* its share of the pack: a 10 ml pack listed
/// at 1,00 EUR sells for 9,13 EUR net, yet 5 ml of it is 1,00 EUR against 4,57 EUR for half the
/// pack. `policy.subset_proportional_floor` lifts the Teilmenge to that share.
///
/// **The floor goes beyond the 100 % the analogy allows** — 4,57 EUR on a pro-rata basis of
/// 0,50 EUR is a 357 % surcharge. How far beyond *the law* is less clear than it looks: the 100 %
/// itself comes from § 4 by analogy, on a case the Bundestierärztekammer calls "lückenhaft", so
/// there is no crisp statutory ceiling here to point at. Treat it as a departure from established
/// practice rather than a proven breach. Off by default and switched on per installation in
/// `[pharmacy]`, as a deliberate decision by the practice — not something this code should do on
/// its own.
///
/// The floor applies whatever the rule, which costs one branch less than restricting it to human
/// preparations; for a veterinary medicine it provably never binds, because the bands stay below
/// 100 % and so § 4 always already exceeds the pro-rata share.
pub fn drug_price_subset(
    original_list_price_net: Decimal,
    original_quantity: Decimal,
    subset_quantity: Decimal,
    vat_percent: Decimal,
    policy: PricingPolicy,
) -> DrugPrice {
    if original_quantity <= Decimal::ZERO || subset_quantity <= Decimal::ZERO {
        return price_from(Decimal::ZERO, Decimal::ZERO, vat_percent);
    }
    let basis = original_list_price_net / original_quantity * subset_quantity;
    let mut surcharge = basis;

    if policy.subset_proportional_floor {
        let pack = drug_price_original(original_list_price_net, vat_percent, policy);
        let share = pack.net / original_quantity * subset_quantity;
        surcharge = surcharge.max(share - basis);
    }

    price_from(basis, surcharge, vat_percent)
}

/// Pro-rata net listed price of a subset — what a subset packaging stores as its own
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
    // Rounding happens once, on the net price the law computes and the catalogue stores; the
    // gross follows from it and is what the customer is shown.
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

    /// The invoice's Einzelpreis is `line_total` at quantity 1 (see `TreatmentItem.price_gross`),
    /// which is what makes `Einzelpreis × Menge` reconcile with `Gesamt` for the common case.
    /// FR-030 / issues.md: "the Einzelpreis must already be with the factor applied".
    #[test]
    fn unit_price_carries_the_factor_and_reconciles_with_the_line() {
        // GOT-Nr. 16, net 23.62 EUR at the 1.5-fold rate, one animal, 19 % VAT.
        let unit_net = line_total(dec("23.62"), Decimal::ONE, Some(dec("150")));
        assert_eq!(unit_net, dec("35.43"));
        assert_eq!(add_vat(unit_net, dec("19")).gross, dec("42.16"));

        // At quantity 1 the unit price *is* the line: the two roundings coincide.
        let line = line_total(dec("23.62"), Decimal::ONE, Some(dec("150")));
        assert_eq!(add_vat(line, dec("19")).gross, dec("42.16"));

        // At larger quantities the per-unit rounding may not multiply out exactly — the line
        // total stays authoritative, which is why the invoice never re-derives it.
        let three = line_total(dec("23.62"), dec("3"), Some(dec("150")));
        assert_eq!(three, dec("106.29"));
        assert_eq!(unit_net * dec("3"), dec("106.29"));

        // A case where it genuinely drifts: 0.155 per unit rounds to 0.16, but seven of them
        // come to 1.09, not 1.12.
        let unit = line_total(dec("0.31"), Decimal::ONE, Some(dec("50")));
        assert_eq!(unit, dec("0.16"));
        assert_eq!(
            line_total(dec("0.31"), dec("7"), Some(dec("50"))),
            dec("1.09")
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
    fn vat_is_added_to_the_net_amount() {
        let normal = add_vat(dec("100.00"), dec("19"));
        assert_eq!(normal.net, dec("100.00"));
        assert_eq!(normal.vat, dec("19.00"));
        assert_eq!(normal.gross, dec("119.00"));

        let reduced = add_vat(dec("100.00"), dec("7"));
        assert_eq!(reduced.vat, dec("7.00"));
        assert_eq!(reduced.gross, dec("107.00"));
    }

    #[test]
    fn adding_vat_rounds_and_stays_consistent() {
        // 29.77 × 19 % = 5.6563 → VAT 5.66, and net + vat must equal gross exactly.
        let split = add_vat(dec("29.77"), dec("19"));
        assert_eq!(split.vat, dec("5.66"));
        assert_eq!(split.gross, dec("35.43"));
        assert_eq!(split.net + split.vat, split.gross);
    }

    #[test]
    fn zero_vat_leaves_the_amount_untouched() {
        let split = add_vat(dec("42.00"), Decimal::ZERO);
        assert_eq!(split.net, dec("42.00"));
        assert_eq!(split.vat, Decimal::ZERO);
        assert_eq!(split.gross, dec("42.00"));
    }

    // ⚠ FOR VET REVIEW (SC-005), item GOT-01 in `review.md`: the GOT publishes **net** fees, so
    // the amount billed is the fee plus VAT. The expected values below are not computed by this
    // project — they are read off the practice's own RE-289, produced by the previous system.
    #[test]
    fn got_positions_are_billed_net_plus_vat() {
        // (GOT number, published net fee, gross amount on RE-289)
        let positions = [
            ("16", "23.62", "28.11"), // Allgemeine Untersuchung mit Beratung, Hund/Katze
            ("17", "15.39", "18.31"), // Allgemeine Untersuchung mit Beratung, Heimsäugetiere
            ("40", "34.50", "41.06"), // Hausbesuch
            ("251", "17.25", "20.53"), // Verband anlegen oder abnehmen
            ("394", "16.50", "19.64"), // Untersuchung der Haut/Wunde
            ("662", "10.26", "12.21"), // Otitis externa, Behandlung, je Seite
        ];
        for (number, net, gross) in positions {
            let line = line_total(dec(net), Decimal::ONE, None);
            assert_eq!(line, dec(net), "GOT {number}: a single unit bills the fee");
            assert_eq!(
                add_vat(line, dec("19")).gross,
                dec(gross),
                "GOT {number} must bill {gross} EUR, not the bare net fee",
            );
        }
    }

    #[test]
    fn vat_summary_groups_lines_by_rate() {
        let groups = vat_summary(&[
            (dec("100.00"), dec("19")),
            (dec("10.00"), dec("19")),
            (dec("100.00"), dec("7")),
        ]);

        assert_eq!(groups.len(), 2, "one group per rate");
        // Highest rate first — the order German invoices use.
        assert_eq!(groups[0].vat_percent, dec("19"));
        assert_eq!(groups[0].net, dec("110.00"));
        assert_eq!(groups[0].vat, dec("20.90"));
        assert_eq!(groups[0].gross, dec("130.90"));
        assert_eq!(groups[1].vat_percent, dec("7"));
        assert_eq!(groups[1].vat, dec("7.00"));
    }

    // ⚠ FOR VET REVIEW (SC-005), item MWS-01 in `review.md`.
    #[test]
    fn vat_is_rounded_per_line_so_the_gross_column_adds_up() {
        // Three lines of 3,33 EUR net at 19 %. Rounded per line the VAT is 0,63 each, so a line
        // is 3,96 gross and the three of them come to exactly the group's 11,88. Rounding on the
        // group total instead would give 19 % of 9,99 = 1,90, a gross of 11,89, and a printed
        // column a cent short of its own total.
        let groups = vat_summary(&[
            (dec("3.33"), dec("19")),
            (dec("3.33"), dec("19")),
            (dec("3.33"), dec("19")),
        ]);
        assert_eq!(groups[0].net, dec("9.99"));
        assert_eq!(groups[0].vat, dec("1.89"), "0,63 per line, three times");
        assert_eq!(groups[0].gross, dec("11.88"));

        let line = add_vat(dec("3.33"), dec("19"));
        assert_eq!(line.gross, dec("3.96"));
        assert_eq!(
            line.gross * dec("3"),
            groups[0].gross,
            "the printed column sums to the group's gross by construction",
        );
    }

    #[test]
    fn total_sums_the_group_gross_amounts() {
        let groups = vat_summary(&[(dec("100.00"), dec("19")), (dec("100.00"), dec("7"))]);
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
        let cheap = drug_price_original(dec("1.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(cheap.surcharge, dec("0.68"));
        assert_eq!(cheap.net, dec("1.68"));
        assert_eq!(cheap.gross, dec("2.00"));

        // § 3(3): von 8,68 bis 12,14 Euro → 48 %. 10.00 + 4.80 = 14.80 net → 17.61 gross.
        let middle = drug_price_original(dec("10.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(middle.surcharge, dec("4.80"));
        assert_eq!(middle.gross, dec("17.61"));

        // § 3(3): von 35,95 bis 543,91 Euro → 30 %. 40.00 + 12.00 = 52.00 → 61.88 gross.
        let upper = drug_price_original(dec("40.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(upper.surcharge, dec("12.00"));
        assert_eq!(upper.gross, dec("61.88"));
    }

    #[test]
    fn the_fixed_amounts_of_ampreisv_3_4_fill_the_gaps_between_the_tiers() {
        // § 3(4): von 1,23 bis 1,34 Euro → 0,83 Euro.
        let first_gap = drug_price_original(dec("1.30"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(first_gap.surcharge, dec("0.83"));
        assert_eq!(first_gap.gross, dec("2.53"));

        // § 3(4): von 19,43 bis 22,57 Euro → 8,35 Euro.
        let fifth_gap = drug_price_original(dec("20.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(fifth_gap.surcharge, dec("8.35"));
        assert_eq!(fifth_gap.gross, dec("33.74"));

        // § 3(4): von 29,15 bis 35,94 Euro → 10,78 Euro.
        let last_gap = drug_price_original(dec("30.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(last_gap.surcharge, dec("10.78"));
    }

    #[test]
    fn every_tier_boundary_is_covered_exactly_once() {
        // The bands of § 3(3) and § 3(4) partition the range; walking the boundaries must
        // never produce a gap (a missing band would silently price a drug at zero margin).
        for cents in 1..60_000u64 {
            let basis = Decimal::new(i64::try_from(cents).unwrap_or(i64::MAX), 2);
            let price = drug_price_original(basis, Decimal::ZERO, PricingPolicy::veterinary());
            assert!(
                price.surcharge > Decimal::ZERO,
                "no surcharge band covers a basis of {basis}"
            );
            assert!(
                price.net > basis,
                "the net price must exceed the listed price"
            );
        }
    }

    #[test]
    fn expensive_drugs_fall_under_the_reduced_rates_of_ampreisv_10_2() {
        // § 10(2): 30 % on the first 51.13 (§ 3(3) band) plus 25 % of the excess up to
        // 127.82. 15.339 + 12.2175 = 27.5565 → net 127.5565 → gross 151.79.
        let hundred = drug_price_original(dec("100.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(hundred.surcharge, dec("27.56"));
        assert_eq!(hundred.gross, dec("151.79"));

        // § 10(2): plus 20 % of the part above 127.82.
        // 15.339 + 19.1725 + 14.436 = 48.9475 → net 248.9475 → gross 296.25.
        let expensive = drug_price_original(dec("200.00"), dec("19"), PricingPolicy::veterinary());
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
        // A cent more on the listed price must never mean a cent less sales price.
        let mut previous = Decimal::ZERO;
        for cents in 1..20_000u64 {
            let basis = Decimal::new(i64::try_from(cents).unwrap_or(i64::MAX), 2);
            let net = drug_price_original(basis, Decimal::ZERO, PricingPolicy::veterinary()).net;
            assert!(net >= previous, "the net price fell at a basis of {basis}");
            previous = net;
        }
    }

    // ⚠ FOR VET REVIEW (SC-005): a human preparation used on an animal is priced by § 3 Abs. 1
    // Satz 2 — "höchstens ein Zuschlag von 3 Prozent zuzüglich 8,10 Euro" — not by the veterinary
    // bands of Abs. 3/4. Until migration `0011` every drug took the bands, which was wrong in both
    // directions: far too little on a cheap preparation, more than the law allows on a dear one.
    #[test]
    fn a_human_preparation_is_priced_by_ampreisv_3_1_s2() {
        let human = PricingPolicy {
            rule: DrugRule::Human,
            ..PricingPolicy::default()
        };
        // (listed price, the Satz 2 surcharge, what the veterinary bands would have charged)
        let cases = [
            ("1.00", "8.13", "0.68"),
            ("10.00", "8.40", "4.80"),
            ("100.00", "11.10", "27.56"),
        ];
        for (list, satz2, bands) in cases {
            let price = drug_price_original(dec(list), dec("19"), human);
            assert_eq!(
                price.surcharge,
                dec(satz2),
                "a human preparation listed at {list} carries 3 % + 8,10 EUR",
            );
            assert_eq!(
                drug_price_original(dec(list), dec("19"), PricingPolicy::veterinary()).surcharge,
                dec(bands),
                "and the veterinary bands would have charged something else entirely",
            );
        }
    }

    #[test]
    fn the_reduced_rates_of_10_2_never_bind_for_a_human_preparation() {
        // § 10(2) caps the part above 51,13 EUR at 25 % and then 20 %. Satz 2 charges 3 %, so the
        // cap can never be the binding one — asserted rather than argued.
        let human = PricingPolicy {
            rule: DrugRule::Human,
            ..PricingPolicy::default()
        };
        for list in ["51.13", "127.82", "500.00", "5000.00"] {
            let satz2 = drug_price_original(dec(list), dec("19"), human).surcharge;
            let capped =
                drug_price_original(dec(list), dec("19"), PricingPolicy::veterinary()).surcharge;
            assert!(
                satz2 < capped,
                "at {list} EUR Satz 2 charges {satz2}, § 10(2) would allow {capped}",
            );
        }
    }

    // ⚠ FOR VET REVIEW (SC-005): why the floor switch exists.
    #[test]
    fn a_cheap_human_teilmenge_falls_below_the_proportional_price() {
        let human = PricingPolicy {
            rule: DrugRule::Human,
            subset_proportional_floor: false,
        };
        // A 10 ml pack listed at 1,00 EUR: the pack carries the flat 8,10 EUR, the Teilmenge only
        // the § 4 percentage, so 5 ml costs a fifth of what half the pack does.
        let pack = drug_price_original(dec("1.00"), dec("19"), human);
        assert_eq!(pack.net, dec("9.13"));

        let half = drug_price_subset(dec("1.00"), dec("10"), dec("5"), dec("19"), human);
        assert_eq!(half.net, dec("1.00"), "§ 4: 0,50 pro rata plus 100 %");
        assert!(
            half.net < pack.net / dec("2"),
            "1,00 EUR for 5 ml against 4,57 EUR for half the pack",
        );
    }

    #[test]
    fn the_proportional_floor_lifts_a_cheap_human_teilmenge() {
        let floored = PricingPolicy {
            rule: DrugRule::Human,
            subset_proportional_floor: true,
        };
        let half = drug_price_subset(dec("1.00"), dec("10"), dec("5"), dec("19"), floored);
        assert_eq!(half.net, dec("4.57"), "half of the pack's 9,13 EUR");
        // The basis is still the pro-rata listed price; only the surcharge was lifted.
        assert_eq!(half.basis_net, dec("0.50"));
        assert_eq!(half.surcharge, dec("4.07"));
    }

    #[test]
    fn the_floor_never_binds_for_a_veterinary_drug() {
        // § 4 charges 100 % while the bands top out at 68 %, so a Teilmenge already exceeds its
        // share of the pack — switching the floor on must change nothing.
        let plain = PricingPolicy::veterinary();
        let floored = PricingPolicy {
            subset_proportional_floor: true,
            ..PricingPolicy::veterinary()
        };
        for list in ["1.00", "10.00", "40.00", "100.00", "600.00"] {
            assert_eq!(
                drug_price_subset(dec(list), dec("100"), dec("10"), dec("19"), plain),
                drug_price_subset(dec(list), dec("100"), dec("10"), dec("19"), floored),
                "the floor must not touch a veterinary drug listed at {list}",
            );
        }
    }

    #[test]
    fn subset_packagings_carry_the_teilmengenzuschlag_of_ampreisv_4() {
        // § 4(1)–(2): the basis is the pro-rata listed price of the dispensed quantity
        // (the usual pack's price is decisive), the surcharge is 100 % (margin 50 %).
        // 10 ml out of a 100 ml bottle bought for 10.00 → basis 1.00 → net 2.00 → 2.38.
        let subset = drug_price_subset(
            dec("10.00"),
            dec("100"),
            dec("10"),
            dec("19"),
            PricingPolicy::veterinary(),
        );
        assert_eq!(subset.basis_net, dec("1.00"));
        assert_eq!(subset.surcharge, dec("1.00"));
        assert_eq!(subset.net, dec("2.00"));
        assert_eq!(subset.gross, dec("2.38"));

        // A half pack: basis 5.00 → net 10.00 → 11.90 gross.
        let half = drug_price_subset(
            dec("10.00"),
            dec("100"),
            dec("50"),
            dec("19"),
            PricingPolicy::veterinary(),
        );
        assert_eq!(half.net, dec("10.00"));
        assert_eq!(half.gross, dec("11.90"));

        // The reduced VAT rate is applied just as faithfully.
        let reduced = drug_price_subset(
            dec("10.00"),
            dec("100"),
            dec("10"),
            dec("7"),
            PricingPolicy::veterinary(),
        );
        assert_eq!(reduced.gross, dec("2.14"));
    }

    #[test]
    fn a_subset_of_the_whole_pack_costs_more_than_the_pack_itself() {
        // Dispensing 100 ml as a "subset" is priced per § 4 (100 %), the whole bottle per
        // § 3(3) (48 % in this band) — the surcharge for repackaging is the difference.
        let pack = drug_price_original(dec("10.00"), dec("19"), PricingPolicy::veterinary());
        let all_of_it = drug_price_subset(
            dec("10.00"),
            dec("100"),
            dec("100"),
            dec("19"),
            PricingPolicy::veterinary(),
        );
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
            drug_price_original(Decimal::ZERO, dec("19"), PricingPolicy::veterinary()).gross,
            Decimal::ZERO
        );
        assert_eq!(
            drug_price_subset(
                dec("10.00"),
                dec("100"),
                Decimal::ZERO,
                dec("19"),
                PricingPolicy::veterinary(),
            )
            .gross,
            Decimal::ZERO
        );
        // A packaging without a quantity cannot be priced pro rata.
        assert_eq!(
            drug_price_subset(
                dec("10.00"),
                Decimal::ZERO,
                dec("10"),
                dec("19"),
                PricingPolicy::veterinary(),
            )
            .gross,
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
