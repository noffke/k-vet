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

/// The unit net price for a gross price the vet types on a position (issues.md): the net whose
/// one unit — after the factor, with VAT added as [`add_vat`] adds it — comes to `gross`.
///
/// Net stays what is stored and computed in, so not every gross amount has a net in whole cents
/// behind it: at 19 % no net gives 15,00 € (12,60 → 14,99; 12,61 → 15,01). Then the nearest is
/// taken, and on a tie the lower — the customer is never charged more than was typed. The vet
/// sees the result in the field once it is saved (`review.md`, item MWS-05).
pub fn net_for_gross(
    gross: Decimal,
    vat_percent: Decimal,
    factor_percent: Option<Decimal>,
) -> Decimal {
    let factor = factor_percent.unwrap_or_else(hundred);
    if factor <= Decimal::ZERO || gross <= Decimal::ZERO {
        return Decimal::ZERO.max(gross);
    }
    let unit_gross =
        |net: Decimal| add_vat(line_total(net, Decimal::ONE, Some(factor)), vat_percent).gross;
    let estimate = round_money(gross * hundred() / (hundred() + vat_percent) * hundred() / factor);
    let cent = Decimal::new(1, CENTS);

    // The two roundings (factor, VAT) put the answer within a few cents of the estimate; look
    // at each and keep the closest gross, below rather than above, then the smaller net.
    (-5..=5)
        .map(|step| estimate + cent * Decimal::from(step))
        .filter(|net| *net >= Decimal::ZERO)
        .min_by_key(|net| {
            let reached = unit_gross(*net);
            ((reached - gross).abs(), reached > gross, *net)
        })
        .unwrap_or(estimate)
}

/// What one billing line comes to, derived from its pinned net price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineAmounts {
    /// One unit after the factor, net — what the line editor shows under the gross price.
    pub unit_net: Decimal,
    /// One unit after the factor, gross — the price column of the invoice.
    pub price_gross: Decimal,
    pub line_net: Decimal,
    /// `line_net` plus its VAT: what the customer pays for the line.
    pub line_gross: Decimal,
}

/// The amounts of one line: unit and line total, net and gross. The one place they are derived,
/// so a treatment's lines and a price quote (Preisauskunft) cannot come out differently.
pub fn line_amounts(
    price_net: Decimal,
    quantity: Decimal,
    factor_percent: Option<Decimal>,
    vat_percent: Decimal,
) -> LineAmounts {
    let unit_net = line_total(price_net, Decimal::ONE, factor_percent);
    let line_net = line_total(price_net, quantity, factor_percent);
    LineAmounts {
        unit_net,
        price_gross: add_vat(unit_net, vat_percent).gross,
        line_net,
        line_gross: add_vat(line_net, vat_percent).gross,
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
///
/// Both paragraphs are read as ceilings, so the lower one wins. That matters only for very
/// dear preparations: from 543,92 EUR § 3(3) grows by 8,263 % while the staging of § 10(2) grows
/// by 20 %, and above a listed price of about 931,18 EUR the staging would allow *more* than
/// § 3(3) itself — which § 10(1) ("höchstens Zuschläge entsprechend § 3") does not. § 10(2)
/// exists to lower a vet's margin on dear drugs, not to raise it above the pharmacy's.
fn vet_surcharge(basis: Decimal) -> Decimal {
    let threshold = decimal("51.13");
    let second_threshold = decimal("127.82");
    if basis <= threshold {
        return pharmacy_surcharge(basis);
    }

    let mut staged = pharmacy_surcharge(threshold);
    let middle = basis.min(second_threshold) - threshold;
    staged += middle * decimal("25") / hundred();
    if basis > second_threshold {
        staged += (basis - second_threshold) * decimal("20") / hundred();
    }
    staged.min(pharmacy_surcharge(basis))
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
// Dispensed quantities — whole packs plus a Teilmenge
// ---------------------------------------------------------------------------
//
// A drug line says what was dispensed, in the original pack's base unit (Tbl., ml): 12 tablets
// out of a pack of 10, 150 ml out of a 100 ml bottle, 2,7 ml of an injection. Whole packs are
// billed at the pack price, the rest as a Teilmenge — at the price of the drug's smallest
// Teilmenge, pro rata, which carries the Teilmengenzuschlag (`drug_price_subset`). On the invoice
// the two parts are two rows, each with its own unit price, total and VAT.

/// Unit prices per base unit are shown to four decimals: a millilitre may cost 0,238 €.
const UNIT_PRICE_PLACES: u32 = 4;

/// The Teilmenge price a dispensed remainder is billed at: `size` base units cost `price_net`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubsetPrice {
    pub size: Decimal,
    pub price_net: Decimal,
}

/// How a drug line in dispensed base units is priced — the values pinned on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispensePricing {
    /// Base units in one original pack: 10 Tbl., 100 ml.
    pub pack_size: Decimal,
    /// Net price of one original pack.
    pub pack_price_net: Decimal,
    /// The price of the rest; `None` when the drug has no Teilmenge — whole packs only.
    pub subset: Option<SubsetPrice>,
    /// "Beliebig teilbar": any rest may be dispensed, not only multiples of the Teilmenge.
    pub freely_divisible: bool,
}

/// One printed row of a dispensed line: the whole packs, or the Teilmenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispenseRow {
    /// Packs on the pack row, base units on the Teilmenge row.
    pub quantity: Decimal,
    /// Net price of one pack, or of one base unit (to four decimals).
    pub unit_net: Decimal,
    /// Gross price of one pack, or of one base unit (to four decimals) — the price column.
    pub price_gross: Decimal,
    pub line_net: Decimal,
    /// `line_net` plus its own VAT, rounded per row (*horizontale Berechnung*).
    pub line_gross: Decimal,
}

/// A dispensed line split into what the invoice prints. A row is absent when it would be zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispenseRows {
    pub packs: Option<DispenseRow>,
    pub remainder: Option<DispenseRow>,
}

impl DispenseRows {
    /// The rows that exist, packs first.
    pub fn rows(&self) -> impl Iterator<Item = &DispenseRow> {
        self.packs.iter().chain(self.remainder.iter())
    }

    pub fn line_net(&self) -> Decimal {
        self.rows().map(|row| row.line_net).sum()
    }

    pub fn line_gross(&self) -> Decimal {
        self.rows().map(|row| row.line_gross).sum()
    }
}

/// Why a dispensed quantity cannot be billed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispenseRefusal {
    /// The drug has no Teilmenge and is not "beliebig teilbar": whole packs only.
    WholePacksOnly,
    /// The rest is not a multiple of the drug's smallest Teilmenge.
    SubsetStep,
}

/// Whole packs and the rest: 150 ml from 100 ml bottles is `(1, 50)`.
pub fn dispense_split(pack_size: Decimal, quantity: Decimal) -> (Decimal, Decimal) {
    if pack_size <= Decimal::ZERO || quantity <= Decimal::ZERO {
        return (Decimal::ZERO, quantity.max(Decimal::ZERO));
    }
    let packs = (quantity / pack_size).floor();
    (packs, quantity - packs * pack_size)
}

/// Whether a quantity can be dispensed: the rest after whole packs has to be a multiple of the
/// smallest Teilmenge, unless the drug is "beliebig teilbar"; without a Teilmenge only whole
/// packs can.
pub fn dispense_check(pricing: &DispensePricing, quantity: Decimal) -> Result<(), DispenseRefusal> {
    let (_, rest) = dispense_split(pricing.pack_size, quantity);
    if rest.is_zero() {
        return Ok(());
    }
    let Some(subset) = pricing.subset else {
        return Err(DispenseRefusal::WholePacksOnly);
    };
    if pricing.freely_divisible {
        return Ok(());
    }
    if subset.size > Decimal::ZERO && (rest % subset.size).is_zero() {
        Ok(())
    } else {
        Err(DispenseRefusal::SubsetStep)
    }
}

/// What a newly picked drug starts at: one base unit where that can be dispensed, otherwise the
/// smallest Teilmenge, otherwise one whole pack.
pub fn dispense_preset(pricing: &DispensePricing) -> Decimal {
    if dispense_check(pricing, Decimal::ONE).is_ok() {
        return Decimal::ONE;
    }
    match pricing.subset {
        Some(subset) if dispense_check(pricing, subset.size).is_ok() => subset.size,
        _ => pricing.pack_size,
    }
}

/// The Teilmenge price of a "beliebig teilbar" drug that has no Teilmenge of its own: the whole
/// pack priced as a Teilmenge (§ 4 by analogy, the floor included), which the rest then takes its
/// share of. Every Teilmenge price is linear in its size, so nothing is lost by scaling it.
pub fn whole_pack_as_subset(
    list_price_net: Decimal,
    pack_size: Decimal,
    vat_percent: Decimal,
    policy: PricingPolicy,
) -> Option<SubsetPrice> {
    let price = drug_price_subset(list_price_net, pack_size, pack_size, vat_percent, policy);
    (price.net > Decimal::ZERO).then_some(SubsetPrice {
        size: pack_size,
        price_net: price.net,
    })
}

/// The rows a dispensed quantity is billed as.
///
/// Whole packs: `line_amounts` of the pack price. The rest: its share of the Teilmenge price,
/// `round(rest × price / size)`, with VAT added to that row alone — so each printed row's gross
/// is exactly `net + vat`, and the invoice column still adds up (MWS-01). A rest with no
/// Teilmenge to price it — only reachable through lines converted from the old form, since
/// [`dispense_check`] refuses it — is billed pro rata of the pack, as the old form billed half a
/// pack.
pub fn dispense_rows(
    pricing: &DispensePricing,
    quantity: Decimal,
    vat_percent: Decimal,
) -> DispenseRows {
    let (packs, rest) = dispense_split(pricing.pack_size, quantity);
    let pack_row = (packs > Decimal::ZERO).then(|| {
        let amounts = line_amounts(pricing.pack_price_net, packs, None, vat_percent);
        DispenseRow {
            quantity: packs,
            unit_net: amounts.unit_net,
            price_gross: amounts.price_gross,
            line_net: amounts.line_net,
            line_gross: amounts.line_gross,
        }
    });
    let reference = pricing.subset.unwrap_or(SubsetPrice {
        size: pricing.pack_size,
        price_net: pricing.pack_price_net,
    });
    let remainder_row = (rest > Decimal::ZERO && reference.size > Decimal::ZERO).then(|| {
        let per_unit = reference.price_net / reference.size;
        let line_net = round_money(rest * per_unit);
        let unit_price = |value: Decimal| {
            value.round_dp_with_strategy(UNIT_PRICE_PLACES, RoundingStrategy::MidpointAwayFromZero)
        };
        DispenseRow {
            quantity: rest,
            unit_net: unit_price(per_unit),
            price_gross: unit_price(per_unit * (Decimal::ONE + vat_percent / hundred())),
            line_net,
            line_gross: add_vat(line_net, vat_percent).gross,
        }
    });
    DispenseRows {
        packs: pack_row,
        remainder: remainder_row,
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
    fn line_amounts_derive_unit_and_line_from_the_net_price() {
        // GOT 40 at 2.3×: 17.86 € net per unit, 3 units, 19 % VAT.
        let amounts = line_amounts(dec("17.86"), dec("3"), Some(dec("230")), dec("19"));
        assert_eq!(amounts.unit_net, dec("41.08"));
        assert_eq!(amounts.price_gross, dec("48.89"));
        assert_eq!(amounts.line_net, dec("123.23"));
        assert_eq!(amounts.line_gross, dec("146.64"));
        // A drug line has no factor; 7 % VAT.
        let drug = line_amounts(dec("9.13"), dec("2"), None, dec("7"));
        assert_eq!(drug.unit_net, dec("9.13"));
        assert_eq!(drug.price_gross, dec("9.77"));
        assert_eq!(drug.line_net, dec("18.26"));
        assert_eq!(drug.line_gross, dec("19.54"));
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

    // ⚠ FOR VET REVIEW (SC-005), item MWS-05 in `review.md`: a typed gross price is met to the
    // cent when a net in whole cents gives it, and missed by one when none does.
    #[test]
    fn a_typed_gross_price_finds_its_net() {
        let gross_of = |net: &str, vat: &str, factor: Option<&str>| {
            add_vat(
                line_total(dec(net), Decimal::ONE, factor.map(dec)),
                dec(vat),
            )
            .gross
        };
        assert_eq!(net_for_gross(dec("10.00"), dec("19"), None), dec("8.40"));
        assert_eq!(gross_of("8.40", "19", None), dec("10.00"), "met exactly");
        assert_eq!(net_for_gross(dec("10.70"), dec("7"), None), dec("10.00"));

        // No net gives 15,00 at 19 %: 12,60 → 14,99 and 12,61 → 15,01. The lower one is taken,
        // so the customer is never charged more than was typed.
        assert_eq!(net_for_gross(dec("15.00"), dec("19"), None), dec("12.60"));
        assert_eq!(gross_of("12.60", "19", None), dec("14.99"));
        // The same at 7 %: 11,21 → 11,99 and 11,22 → 12,01.
        assert_eq!(net_for_gross(dec("12.00"), dec("7"), None), dec("11.21"));

        // The gross typed is the unit price *after* the factor, as the invoice prints it:
        // 23,80 € at 200 % is a fee of 10,00 € net, doubled, plus 19 %.
        assert_eq!(
            net_for_gross(dec("23.80"), dec("19"), Some(dec("200"))),
            dec("10.00")
        );
        assert_eq!(net_for_gross(dec("5.00"), dec("0"), None), dec("5.00"));
        assert_eq!(net_for_gross(Decimal::ZERO, dec("19"), None), Decimal::ZERO);

        // Two units of a 10,00 € line are not 20,00 €: the VAT is rounded on the line's net
        // (16,80 € → 3,19 €), as `MWS-01` asks.
        let line = line_total(dec("8.40"), dec("2"), None);
        assert_eq!(add_vat(line, dec("19")).gross, dec("19.99"));
    }

    #[test]
    fn every_gross_a_net_can_give_is_met_exactly_and_the_rest_by_a_cent() {
        for vat in ["19", "7"] {
            let reachable: std::collections::HashSet<Decimal> = (0..=3_000)
                .map(|cents| add_vat(Decimal::new(cents, 2), dec(vat)).gross)
                .collect();
            for cents in 0..=3_000 {
                let gross = Decimal::new(cents, 2);
                let met = add_vat(net_for_gross(gross, dec(vat), None), dec(vat)).gross;
                if reachable.contains(&gross) {
                    assert_eq!(met, gross, "{gross} at {vat} % has a net in whole cents");
                } else {
                    assert_eq!(met, gross - dec("0.01"), "{gross} at {vat} %: a cent below");
                }
            }
        }
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
    fn a_very_dear_drug_is_held_to_the_plain_ampreisv_3_3() {
        // ⚠ FOR VET REVIEW: § 10(1) allows at most the § 3 surcharge, § 10(2) caps the excess
        // further — the lower one wins. From 543,92 EUR § 3(3) is 8,263 % + 118,24 EUR; the
        // staging of § 10(2) grows faster (20 %) and overtakes it at about 931,18 EUR.
        //
        // 800,00: staging 15,339 + 19,1725 + 134,436 = 168,9475 < § 3(3) 184,344 → staging.
        let below = drug_price_original(dec("800.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(below.surcharge, dec("168.95"));
        assert_eq!(below.gross, dec("1153.05"));

        // 1.000,00: staging would be 208,9475; § 3(3) is 82,63 + 118,24 = 200,87 → § 3(3).
        let dear = drug_price_original(dec("1000.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(dear.surcharge, dec("200.87"));
        assert_eq!(dear.net, dec("1200.87"));
        assert_eq!(dear.gross, dec("1429.04"));

        // Across the hand-over, a cent more on the list never means less to charge.
        let mut previous = Decimal::ZERO;
        for cents in 90_000..100_000u64 {
            let basis = Decimal::new(i64::try_from(cents).unwrap_or(i64::MAX), 2);
            let price = drug_price_original(basis, Decimal::ZERO, PricingPolicy::veterinary());
            assert!(
                price.net >= previous,
                "the price fell at a basis of {basis}"
            );
            previous = price.net;
        }
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

    // ── Dispensed quantities: whole packs plus a Teilmenge ──────────────────────────
    //
    // ⚠ FOR VET REVIEW (SC-005), item AMP-06 in `review.md`. The bottle of AMP-04: 100 ml listed
    // at 10,00 € — the pack sells for 14,80 € net (17,61 € gross), its 10 ml Teilmenge for
    // 2,00 € net (2,38 € gross).

    fn bottle(subset: Option<(&str, &str)>, freely_divisible: bool) -> DispensePricing {
        DispensePricing {
            pack_size: dec("100"),
            pack_price_net: dec("14.80"),
            subset: subset.map(|(size, price)| SubsetPrice {
                size: dec(size),
                price_net: dec(price),
            }),
            freely_divisible,
        }
    }

    fn gross(rows: &DispenseRows) -> (Option<Decimal>, Option<Decimal>) {
        (
            rows.packs.map(|row| row.line_gross),
            rows.remainder.map(|row| row.line_gross),
        )
    }

    #[test]
    fn a_dispensed_quantity_is_billed_as_whole_packs_plus_a_teilmenge() {
        let pricing = bottle(Some(("10", "2.00")), false);
        // The pack's own price follows AMP-01.
        assert_eq!(
            drug_price_original(dec("10.00"), dec("19"), PricingPolicy::veterinary()).net,
            dec("14.80")
        );

        // 150 ml: one bottle, and 50 ml as five Teilmengen of 10 ml.
        let rows = dispense_rows(&pricing, dec("150"), dec("19"));
        let packs = rows.packs.expect("a pack row");
        assert_eq!(
            (packs.quantity, packs.price_gross),
            (dec("1"), dec("17.61"))
        );
        let rest = rows.remainder.expect("a Teilmenge row");
        assert_eq!(rest.quantity, dec("50"));
        assert_eq!(rest.unit_net, dec("0.2"));
        assert_eq!(rest.price_gross, dec("0.238"));
        assert_eq!(
            (rest.line_net, rest.line_gross),
            (dec("10.00"), dec("11.90"))
        );
        assert_eq!(rows.line_gross(), dec("29.51"));
        assert_eq!(rows.line_net(), dec("24.80"));

        // Less than a bottle is the Teilmenge alone; whole bottles are the pack row alone.
        let ten = dispense_rows(&pricing, dec("10"), dec("19"));
        assert_eq!(gross(&ten), (None, Some(dec("2.38"))));
        let two = dispense_rows(&pricing, dec("200"), dec("19"));
        assert_eq!(gross(&two), (Some(dec("35.22")), None));
        assert_eq!(two.packs.map(|row| row.quantity), Some(dec("2")));
    }

    #[test]
    fn twelve_tablets_from_a_pack_of_ten_are_a_pack_and_two_tablets() {
        // 10 tablets listed at 20,00 € fall into the fixed band of § 3(4): 8,35 € → 28,35 € net.
        let pack = drug_price_original(dec("20.00"), dec("19"), PricingPolicy::veterinary());
        assert_eq!(pack.net, dec("28.35"));
        // A Teilmenge of one tablet: 2,00 € pro rata plus 100 % → 4,00 € net.
        let tablet = drug_price_subset(
            dec("20.00"),
            dec("10"),
            dec("1"),
            dec("19"),
            PricingPolicy::veterinary(),
        );
        assert_eq!(tablet.net, dec("4.00"));

        let pricing = DispensePricing {
            pack_size: dec("10"),
            pack_price_net: pack.net,
            subset: Some(SubsetPrice {
                size: dec("1"),
                price_net: tablet.net,
            }),
            freely_divisible: false,
        };
        let rows = dispense_rows(&pricing, dec("12"), dec("19"));
        assert_eq!(gross(&rows), (Some(dec("33.74")), Some(dec("9.52"))));
        assert_eq!(rows.line_gross(), dec("43.26"));
        assert_eq!(dispense_check(&pricing, dec("12")), Ok(()));
    }

    #[test]
    fn a_freely_divisible_drug_takes_any_quantity() {
        // 2,7 ml of an injection: 2,7 × 0,20 € = 0,54 € net, 0,64 € gross — the same with the
        // 10 ml Teilmenge as with the whole bottle priced as a Teilmenge, because both are pro
        // rata of the listed price.
        let with_subset = bottle(Some(("10", "2.00")), true);
        let whole = whole_pack_as_subset(
            dec("10.00"),
            dec("100"),
            dec("19"),
            PricingPolicy::veterinary(),
        );
        assert_eq!(
            whole,
            Some(SubsetPrice {
                size: dec("100"),
                price_net: dec("20.00"),
            })
        );
        let without_subset = DispensePricing {
            subset: whole,
            ..bottle(None, true)
        };
        for pricing in [with_subset, without_subset] {
            assert_eq!(dispense_check(&pricing, dec("2.7")), Ok(()));
            let rows = dispense_rows(&pricing, dec("2.7"), dec("19"));
            assert_eq!(gross(&rows), (None, Some(dec("0.64"))));
            assert_eq!(dispense_preset(&pricing), dec("1"));
        }
        // 150 ml: a bottle and 50 ml, exactly as for the drug with steps.
        let rows = dispense_rows(&without_subset, dec("150"), dec("19"));
        assert_eq!(gross(&rows), (Some(dec("17.61")), Some(dec("11.90"))));
        // Without a listed price there is no Teilmenge price to take a share of.
        assert_eq!(
            whole_pack_as_subset(
                Decimal::ZERO,
                dec("100"),
                dec("19"),
                PricingPolicy::veterinary()
            ),
            None
        );
    }

    #[test]
    fn a_rest_that_does_not_fit_the_teilmenge_is_refused() {
        let steps_of_ten = bottle(Some(("10", "2.00")), false);
        assert_eq!(
            dispense_check(&steps_of_ten, dec("15")),
            Err(DispenseRefusal::SubsetStep)
        );
        assert_eq!(
            dispense_check(&steps_of_ten, dec("2.7")),
            Err(DispenseRefusal::SubsetStep)
        );
        assert_eq!(dispense_check(&steps_of_ten, dec("130")), Ok(()));

        // A blister of five: 12 tablets are a pack and two, and two is not a blister.
        let blisters = DispensePricing {
            pack_size: dec("10"),
            pack_price_net: dec("28.35"),
            subset: Some(SubsetPrice {
                size: dec("5"),
                price_net: dec("20.00"),
            }),
            freely_divisible: false,
        };
        assert_eq!(
            dispense_check(&blisters, dec("12")),
            Err(DispenseRefusal::SubsetStep)
        );
        assert_eq!(dispense_check(&blisters, dec("15")), Ok(()));

        // No Teilmenge at all: whole packs only.
        let packs_only = bottle(None, false);
        assert_eq!(
            dispense_check(&packs_only, dec("150")),
            Err(DispenseRefusal::WholePacksOnly)
        );
        assert_eq!(dispense_check(&packs_only, dec("300")), Ok(()));
    }

    #[test]
    fn a_new_line_starts_at_one_unit_where_one_unit_can_be_dispensed() {
        let tablets = DispensePricing {
            pack_size: dec("10"),
            pack_price_net: dec("28.35"),
            subset: Some(SubsetPrice {
                size: dec("1"),
                price_net: dec("4.00"),
            }),
            freely_divisible: false,
        };
        assert_eq!(dispense_preset(&tablets), dec("1"));
        // Steps of 10 ml: one step. No Teilmenge: one bottle.
        assert_eq!(
            dispense_preset(&bottle(Some(("10", "2.00")), false)),
            dec("10")
        );
        assert_eq!(dispense_preset(&bottle(None, false)), dec("100"));
        // A pack of one is one pack.
        let tube = DispensePricing {
            pack_size: dec("1"),
            pack_price_net: dec("8.00"),
            subset: None,
            freely_divisible: false,
        };
        assert_eq!(dispense_preset(&tube), dec("1"));
    }

    #[test]
    fn a_teilmenge_unit_price_shows_four_decimals_and_the_row_stays_in_cents() {
        // A blister of five at 11,87 € net: 2,374 € per tablet, 2,8251 € gross. Two blisters are
        // 23,74 € net — the Teilmenge price twice, not the rounded unit price ten times.
        let pricing = DispensePricing {
            pack_size: dec("20"),
            pack_price_net: dec("40.00"),
            subset: Some(SubsetPrice {
                size: dec("5"),
                price_net: dec("11.87"),
            }),
            freely_divisible: false,
        };
        let rows = dispense_rows(&pricing, dec("10"), dec("19"));
        let rest = rows.remainder.expect("a Teilmenge row");
        assert_eq!(rest.unit_net, dec("2.374"));
        assert_eq!(rest.price_gross, dec("2.8251"));
        assert_eq!(rest.line_net, dec("23.74"));
        assert_eq!(rest.line_gross, dec("28.25"));
    }

    #[test]
    fn the_rows_add_up_to_the_vat_summary() {
        // Every row's gross is its net plus its own VAT, so the rows feed `vat_summary` as they
        // are and the printed column adds up (MWS-01).
        let pricing = bottle(Some(("10", "2.00")), false);
        for quantity in ["10", "90", "100", "110", "150", "250", "370"] {
            let rows = dispense_rows(&pricing, dec(quantity), dec("19"));
            let summary = vat_summary(
                &rows
                    .rows()
                    .map(|row| (row.line_net, dec("19")))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(total_gross(&summary), rows.line_gross(), "{quantity} ml");
        }
    }

    #[test]
    fn a_converted_rest_without_a_teilmenge_is_billed_pro_rata() {
        // Half a bottle, as the old form could bill it: half the pack price.
        let rows = dispense_rows(&bottle(None, false), dec("50"), dec("19"));
        assert_eq!(rows.remainder.map(|row| row.line_net), Some(dec("7.40")));
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
