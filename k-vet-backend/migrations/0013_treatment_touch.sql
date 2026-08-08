-- A treatment's own row does not change when its lines do, so nothing could tell whether the
-- invoice PDF still matches what is billed: the PDF is rendered once, at creation, while the
-- totals are recomputed live from the lines on every read.
--
-- A deleted line is the case that rules out doing this in the application: there is no row
-- left to carry a timestamp. The parent is touched here instead, for every write.

CREATE FUNCTION touch_treatment() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE treatment
       SET updated_at = now()
     WHERE id = COALESCE(NEW.treatment_id, OLD.treatment_id);
    RETURN NULL;
END;
$$;

CREATE TRIGGER treatment_item_touches_treatment
    AFTER INSERT OR UPDATE OR DELETE ON treatment_item
    FOR EACH ROW EXECUTE FUNCTION touch_treatment();

-- The invoice names every animal it bills, so attaching or detaching one dates the PDF too.
CREATE TRIGGER treatment_patient_touches_treatment
    AFTER INSERT OR DELETE ON treatment_patient
    FOR EACH ROW EXECUTE FUNCTION touch_treatment();
