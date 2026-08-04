-- An optional weight on a patient, in kilograms.
--
-- Scale 1 is the requirement — "at most one fractional digit" — enforced here rather than only in
-- the form, so the column cannot hold something the UI would never produce. Precision 5 reaches
-- 9999,9 kg, which covers everything from a rabbit to the landwirtschaftliche Nutztiere the GOT
-- catalogue carries.
--
-- Note what the vet actually sees: the form rounds to one decimal as she types, the way every
-- other number field in this application does, so 4,25 kg becomes 4,3 kg before it is ever sent.
-- The API additionally refuses a second decimal outright, which only a non-UI client can trigger —
-- there the alternative would be Postgres rounding it away with nobody told.
--
-- Named `weight_kg`, not `weight`: the unit should not be something a reader has to go and look up.
--
-- Deliberately a single **current** value on the patient rather than one per treatment. That is
-- what was asked for and it is the simpler model, but it means each weighing overwrites the last:
-- there is no history, and no way to see that a cat has lost 400 g since spring. Moving it to
-- `treatment` later is a migration, not an edit.
--
-- Nullable and absent from `patient_complete` on purpose — the weight is optional, and requiring
-- it would put every existing patient back into draft.

ALTER TABLE patient ADD COLUMN weight_kg NUMERIC(5, 1);

ALTER TABLE patient ADD CONSTRAINT patient_weight_positive
    CHECK (weight_kg IS NULL OR weight_kg > 0);
