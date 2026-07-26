# Drug Price Calculation
Sale prices of drugs are calculated based on the list price and the rules given in AMPreisV https://www.gesetze-im-internet.de/ampreisv/BJNR021470980.html .
Only the part about veterinarians (Tierärzte) is relevant. §4 also gives the surcharge for non original packaging units (Teilmengenzuschlag).

## Design
Decision: v1 calculates per AMPreisV only (veterinarian part, incl. §4 Teilmengenzuschlag for subsets); the computed gross price remains manually overridable per packaging. A configurable "list price + VAT" mode is deferred.