# Drug Price Calculation
Sale prices of drugs are calculated based on the list price and the rules given in AMPreisV https://www.gesetze-im-internet.de/ampreisv/BJNR021470980.html .
Only the part about veterinarians (Tierärzte) is relevant. §4 also gives the surcharge for non original packaging units (Teilmengenzuschlag).

## Design
Decision: v1 calculates per AMPreisV only (veterinarian part, incl. §4 Teilmengenzuschlag for subsets); the computed gross price remains manually overridable per packaging. A configurable "list price + VAT" mode is deferred.
## Net is the stored unit

Prices are stored and edited **net**; the gross is derived. This follows the law rather than
convenience: the GOT publishes net fees, AMPreisV § 3(2) levies its surcharges on the net purchase
price, and § 14 UStG states an invoice as Entgelt plus Steuerbetrag. Storing gross also lets a VAT
rate change silently move the practice's margin, because the old rate stays baked into the figure.

It was the other way round until migration `0009`. Because the GOT catalogue had been imported with
its (net) published fees into a column named `gross_price`, every GOT position was billed roughly
16 % too low — GOT 16 at 23,62 € where 28,11 € is correct. See `review.md`, item `GOT-01`.

A private customer reads gross amounts, so the invoice prints gross columns. Since gross is derived,
the odd cent is allocated across a group's lines so the printed column sums to the group's gross
exactly; never re-derive a gross amount by multiplying a net one.
