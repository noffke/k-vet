-- Human preparations get their own AMPreisV rule, and a treatment line can be redesignated.
--
-- § 3 Abs. 1 AMPreisV has one Satz per case, and § 10 Abs. 1 cites exactly those two:
--
--   Satz 2: "Soweit Fertigarzneimittel, die zur Anwendung bei Menschen bestimmt sind, durch die
--            Apotheken zur Anwendung bei Tieren abgegeben werden, dürfen zur Berechnung des
--            Apothekenabgabepreises abweichend von Satz 1 höchstens ein Zuschlag von 3 Prozent
--            zuzüglich 8,10 Euro sowie die Umsatzsteuer erhoben werden."
--   Satz 3: "Bei der Abgabe von Fertigarzneimitteln, die zur Anwendung bei Tieren bestimmt sind,
--            durch die Apotheken dürfen zur Berechnung des Apothekenabgabepreises höchstens
--            Zuschläge nach Absatz 3 oder 4 sowie die Umsatzsteuer erhoben werden."
--
-- Until now every drug was priced by the Satz 3 bands, which is wrong in both directions for a
-- human preparation: at a listed price of 1,00 € it charged 0,68 € where 8,13 € is allowed, and at
-- 100,00 € it charged 27,56 € where only 11,10 € is. `human_drug` selects the rule.
--
-- `treatment_item.redesignation` is the ad-hoc counterpart of `drug.redesignation`: an eye
-- preparation used in an ear is redesignated for that one treatment, not for the catalogue entry.
-- Like the drug-level flag it is documentation only and has no effect on the price.
--
-- Both default to false, which keeps every existing drug on the veterinary rule and marks no
-- existing line as redesignated.

ALTER TABLE drug           ADD COLUMN human_drug    BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE treatment_item ADD COLUMN redesignation BOOLEAN NOT NULL DEFAULT false;

-- Only a dispensed drug can be redesignated; a GOT position cannot.
ALTER TABLE treatment_item ADD CONSTRAINT treatment_item_redesignation_is_a_drug
    CHECK (kind = 'drug_packaging' OR NOT redesignation);
