# Drug Price Calculation
Sale prices of drugs are calculated based on the list price and the rules given in AMPreisV https://www.gesetze-im-internet.de/ampreisv/BJNR021470980.html .
Only the part about veterinarians (Tierärzte) is relevant. §4 also gives the surcharge for non original packaging units (Teilmengenzuschlag).

## Design
Decision: v1 calculates per AMPreisV only (veterinarian part, incl. §4 Teilmengenzuschlag for subsets); the computed gross price remains manually overridable per packaging. A configurable "list price + VAT" mode is deferred.
## The AMPreisV basis is a listed price

Drug surcharges are levied on the **Listenpreis**, not on what the practice actually paid. § 3
Abs. 2 AMPreisV names the "Abgabepreis des pharmazeutischen Unternehmers ohne die Umsatzsteuer"
plus the § 2 wholesale surcharge, and § 10 Abs. 2 refers back to exactly that figure — it never
says *Einkaufspreis*. A negotiated rebate therefore does not lower what may be charged on.

The practice buys from a wholesaler, so the § 2 surcharge is already inside every price it sees:
`drug_packaging.list_price_net` holds the wholesaler's listed net price and nothing has to be
computed on top. Note that veterinary price lists (Barsoi among them) label this same figure
"Einkaufspreis" — in AMPreisV usage that word means the *listed* purchase price, which is why the
column is named `list_price_net` instead.

## Two AMPreisV rules, chosen per drug

§ 3 Abs. 1 AMPreisV has one Satz per case, and § 10 Abs. 1 lets a vet use both:

* **Satz 3 — veterinary medicine.** The percentage bands of Abs. 3 and the fixed amounts of Abs. 4,
  capped for expensive preparations by § 10 Abs. 2. This is the default.
* **Satz 2 — human medicine used on an animal ("Umwidmung").** "höchstens ein Zuschlag von
  3 Prozent zuzüglich 8,10 Euro". Flat where the bands are proportional, so it is far more than the
  bands on a cheap preparation and far less on a dear one; the two cross around 20–30 € of listed
  price. § 10 Abs. 2 cannot bind on top, because 3 % never reaches its 25 %/20 % ceiling.

`drug.human_drug` selects the rule. Everything before migration `0011` used the bands for every
drug, which undercharged a preparation listed at 1,00 € (0,68 € instead of 8,13 €) and overcharged
one at 100,00 € (27,56 € instead of 11,10 €).

## The Teilmengenzuschlag rests on an analogy, not on the statute

§ 4 AMPreisV governs a *Stoff*, which AMG § 3 defines as a chemical element or compound, a plant,
animal material or a microorganism — table salt, not a pack of Carprofen. A part-pack taken from a
finished medicine is a Fertigarzneimittel (AMG § 4 Abs. 1), so § 4 does not literally reach it.

What settles it is practice, not text. The Bundestierärztekammer wrote to the BMG on 2018-12-14
that "die Preisberechnung für aus Fertigarzneimitteln entnommene Teilmengen erfolgt derzeit **in
Anlehnung an § 4 AMPreisV** … ein Festzuschlag von 100 Prozent sowie die Umsatzsteuer", while
stating that the regulation "ist in diesem Punkt **lückenhaft**" and asking for the gap to be
closed. § 10 Abs. 1's "entsprechend" is what carries the analogy across.

Two consequences. First, do not rewrite this rule from the statute alone — the statute does not
contain it. Second, § 1 Abs. 3 Nr. 7 exempts Teilmengen dispensed on a *human* prescription from
the Apotheken price spans entirely, but per the BMWi that exemption does **not** extend to
tierärztliche Hausapotheken, so a vet stays bound by the maxima.

## The Teilmengen floor is a deliberate deviation

For a human preparation the whole pack carries the flat 8,10 € while a § 4 Teilmenge carries only a
percentage, so a cheap one comes out below its share of the pack: a 10 ml pack listed at 1,00 €
sells for 9,13 € net, yet 5 ml of it is 1,00 € against 4,57 € for half the pack.

`[pharmacy] subset_never_below_proportional` lifts the Teilmenge to that share. It is **off by
default and exceeds the statutory maximum**: § 10 Abs. 1 permits *höchstens* the § 4 surcharge of
100 %, and 4,57 € on a basis of 0,50 € is 357 %. It is a decision for the practice, confirmed with
the vet, not a default of this application. It never bites on veterinary medicines, whose bands stay
below 100 %.

## Net is the stored unit

Prices are stored and edited **net**; the gross is derived. This follows the law rather than
convenience: the GOT publishes net fees, AMPreisV § 3(2) levies its surcharges on the net purchase
price, and § 14 UStG states an invoice as Entgelt plus Steuerbetrag. Storing gross also lets a VAT
rate change silently move the practice's margin, because the old rate stays baked into the figure.

It was the other way round until migration `0009`. Because the GOT catalogue had been imported with
its (net) published fees into a column named `gross_price`, every GOT position was billed roughly
16 % too low — GOT 16 at 23,62 € where 28,11 € is correct. See `review.md`, item `GOT-01`.

A private customer reads gross amounts, so the invoice prints gross columns. To keep that column
honest, VAT is rounded **per line** and summed — the *horizontale Berechnung*, one of the two
roundings German practice accepts under § 14 UStG; the other sums the raw amounts and rounds the
total. Rounding per line makes a line's gross exactly `net + vat`, so the column adds up to the
group's gross by construction. The price is that a group's VAT is the sum of its lines' VAT rather
than the rate applied to the group's net, which can differ by a cent on a long invoice — inherent
to the method and accepted. § 14 Abs. 4 UStG requires the Entgelt per rate (Nr. 7) and the
Steuerbetrag on it (Nr. 8); it prescribes neither rounding, and it does not require a per-line
gross at all.
