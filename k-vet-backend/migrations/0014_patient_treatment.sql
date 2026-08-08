-- A treatment covers a visit; each animal in it gets its own record.
--
-- Until now the reason and the finding were written once for the whole treatment, and a
-- position named its animal in a nullable `patient_id`. That made the attribution a field
-- that could be cleared rather than the structure it has to be: a dispense movement carries
-- no patient of its own — `drug_stock_movement` links only to the line — so which animal
-- received which batch is derived entirely from the line's owner.
--
-- Positions therefore belong to a `patient_treatment`. They keep their `treatment_id` as
-- well, because a visit also has lines that belong to no single animal: the Wegegeld of a
-- house call for two of them.

CREATE TABLE patient_treatment (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    treatment_id     BIGINT NOT NULL REFERENCES treatment (id) ON DELETE CASCADE,
    patient_id       BIGINT NOT NULL REFERENCES patient (id),
    treatment_reason TEXT,
    finding          TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT patient_treatment_once_per_animal UNIQUE (treatment_id, patient_id),
    -- The target of the composite foreign key below: a position's animal record must belong
    -- to the position's own treatment.
    CONSTRAINT patient_treatment_within_treatment UNIQUE (treatment_id, id)
);

CREATE TRIGGER patient_treatment_set_updated_at BEFORE UPDATE ON patient_treatment
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX patient_treatment_patient_idx ON patient_treatment (patient_id);

-- One record per animal already on a treatment, carrying what was written for all of them.
INSERT INTO patient_treatment (treatment_id, patient_id, treatment_reason, finding, created_at)
SELECT link.treatment_id, link.patient_id, treatment.treatment_reason, treatment.finding,
       link.created_at
  FROM treatment_patient link
  JOIN treatment ON treatment.id = link.treatment_id;

ALTER TABLE treatment_item ADD COLUMN patient_treatment_id BIGINT;

UPDATE treatment_item item
   SET patient_treatment_id = record.id
  FROM patient_treatment record
 WHERE record.treatment_id = item.treatment_id
   AND record.patient_id = item.patient_id;

-- Positions were numbered across the whole treatment and are now numbered within their owner.
WITH renumbered AS (
    SELECT id,
           row_number() OVER (PARTITION BY treatment_id, patient_treatment_id
                              ORDER BY position, id) AS position
      FROM treatment_item
)
UPDATE treatment_item item
   SET position = renumbered.position
  FROM renumbered
 WHERE renumbered.id = item.id;

ALTER TABLE treatment_item
    DROP CONSTRAINT treatment_item_position_key,
    -- NULLS NOT DISTINCT so the lines belonging to no animal are ordered among themselves;
    -- deferred because reordering swaps two positions and collides in between.
    ADD CONSTRAINT treatment_item_position_key
        UNIQUE NULLS NOT DISTINCT (treatment_id, patient_treatment_id, position)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT treatment_item_patient_treatment_fkey
        FOREIGN KEY (treatment_id, patient_treatment_id)
        REFERENCES patient_treatment (treatment_id, id),
    -- A drug is dispensed to an animal, and that is what makes a batch traceable to one.
    DROP CONSTRAINT treatment_item_drug_needs_patient,
    ADD CONSTRAINT treatment_item_drug_needs_patient
        CHECK (kind <> 'drug_packaging' OR patient_treatment_id IS NOT NULL),
    DROP COLUMN patient_id;

CREATE INDEX treatment_item_patient_treatment_idx
    ON treatment_item (patient_treatment_id) WHERE patient_treatment_id IS NOT NULL;

-- Files were hung on the treatment and now hang on the animal's record. Nothing has ever
-- written a treatment_file, so there is nothing to carry over.
-- Dropping `treatment_id` takes every CHECK that mentions it with it, including the one for
-- `patient_file`, so all three shapes are restated here.
ALTER TABLE attachment
    ADD COLUMN patient_treatment_id BIGINT REFERENCES patient_treatment (id) ON DELETE CASCADE,
    DROP CONSTRAINT attachment_treatment_file_shape,
    DROP CONSTRAINT attachment_referenced_shape,
    DROP COLUMN treatment_id,
    ADD CONSTRAINT attachment_patient_file_shape CHECK (
        kind <> 'patient_file'
            OR (patient_id IS NOT NULL AND patient_treatment_id IS NULL)),
    ADD CONSTRAINT attachment_treatment_file_shape CHECK (
        kind <> 'treatment_file'
            OR (patient_treatment_id IS NOT NULL AND patient_id IS NULL)),
    ADD CONSTRAINT attachment_referenced_shape CHECK (
        kind <> 'referenced' OR (patient_id IS NULL AND patient_treatment_id IS NULL));

CREATE INDEX attachment_patient_treatment_idx
    ON attachment (patient_treatment_id) WHERE patient_treatment_id IS NOT NULL;

-- The reason and the finding are the animal's now.
ALTER TABLE treatment DROP COLUMN treatment_reason, DROP COLUMN finding;

-- The invoice names every animal it bills, so the record that holds one dates the PDF.
DROP TRIGGER treatment_patient_touches_treatment ON treatment_patient;
CREATE TRIGGER patient_treatment_touches_treatment
    AFTER INSERT OR UPDATE OR DELETE ON patient_treatment
    FOR EACH ROW EXECUTE FUNCTION touch_treatment();

DROP TABLE treatment_patient;
