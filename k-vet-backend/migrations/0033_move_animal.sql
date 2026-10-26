-- An animal can leave a visit with what was written for it (issues.md): moved to an
-- appointment of its own — the customer calls later and wants the second pet on another
-- invoice — its record changes treatment, and its lines and their draft dispenses, its files
-- and its texts go along. The record's `treatment_id` is what changes; the lines follow by the
-- cascade, so a line can still never belong to an animal of another treatment.
ALTER TABLE treatment_item
    DROP CONSTRAINT treatment_item_patient_treatment_fkey,
    ADD CONSTRAINT treatment_item_patient_treatment_fkey
        FOREIGN KEY (treatment_id, patient_treatment_id)
        REFERENCES patient_treatment (treatment_id, id)
        ON UPDATE CASCADE;

-- A row that moves from one treatment to another changes both: the one it left dates its
-- invoice PDF as much as the one it joined.
CREATE OR REPLACE FUNCTION touch_treatment() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE treatment
       SET updated_at = now()
     WHERE id = COALESCE(NEW.treatment_id, OLD.treatment_id)
        OR (TG_OP = 'UPDATE' AND id = OLD.treatment_id);
    RETURN NULL;
END;
$$;
