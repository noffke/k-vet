-- "pro Termin": whether a line belongs to an animal is decided by its service, once.
--
-- Drug dispensing and treatment are attributed to a single animal; the house-call fee and
-- Wegegeld belong to the visit, whichever animals were seen. Until now the vet chose that per
-- line ("+ Ohne Tier"), at every visit, and could choose wrongly. From here on it is a property
-- of the service, set in the catalogue, and strict: a line of a per-visit service never names
-- an animal, every other line always does.

-- 1. The service flag. Pre-set for Wegegeld (every travel-expense service — GOT § 10 charges it
--    per visit, so it cannot be anything else) and for GOT 40, the Hausbesuch. `got_number` is
--    not unique and a self-defined service may carry one as an analogy (0017), hence the type.
ALTER TABLE service ADD COLUMN per_visit BOOLEAN NOT NULL DEFAULT false;

UPDATE service
   SET per_visit = true
 WHERE travel_expenses
    OR (type = 'got' AND got_number = '40');

ALTER TABLE service
    ADD CONSTRAINT service_travel_is_per_visit CHECK (NOT travel_expenses OR per_visit);

-- 2. The line keeps what its service said when it was added, like its price: changing the flag
--    later must not move lines that exist, least of all invoiced ones. With the snapshot tied to
--    the attribution, a line cannot change between an animal and the visit either.
--
--    Lines that exist keep their place — an old "Ohne Tier" line stays the visit's, an old
--    Hausbesuch entered under an animal stays there. Filling the snapshot in is not an edit:
--    `treatment.updated_at` tells an invoice PDF it is out of date, so the triggers that would
--    bump it are off while it runs (as in 0022).
ALTER TABLE treatment_item DISABLE TRIGGER treatment_item_set_updated_at;
ALTER TABLE treatment_item DISABLE TRIGGER treatment_item_touches_treatment;

ALTER TABLE treatment_item ADD COLUMN per_visit BOOLEAN NOT NULL DEFAULT false;
UPDATE treatment_item SET per_visit = true WHERE patient_treatment_id IS NULL;

ALTER TABLE treatment_item ENABLE TRIGGER treatment_item_set_updated_at;
ALTER TABLE treatment_item ENABLE TRIGGER treatment_item_touches_treatment;

-- Every writer states it from here on.
ALTER TABLE treatment_item ALTER COLUMN per_visit DROP DEFAULT;

ALTER TABLE treatment_item
    ADD CONSTRAINT treatment_item_per_visit_has_no_animal
    CHECK (per_visit = (patient_treatment_id IS NULL));
