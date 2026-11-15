-- A drug that is "Beliebig teilbar" has no Teilmengen (migration 0039, requirements/prices.md).
--
-- Any amount of it can be dispensed, priced as its share of the whole pack priced as a
-- Teilmenge; a Teilmenge beside it would only be a second, possibly different price for the
-- same millilitre. So the two exclude each other: the flag is set once the Teilmengen are
-- deleted, and while it is set none can be added. An archived Teilmenge — one a billed line
-- still names, deleted from the drug's page — does not count.
--
-- It spans two tables, which no CHECK can, so a trigger on each holds it. The constraint names
-- are what the API turns into field errors (`error.rs::constraint_field_message`).

CREATE FUNCTION drug_packaging_no_subset_when_freely_divisible() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.kind = 'subset' AND NOT NEW.archived
       AND EXISTS (SELECT 1 FROM drug WHERE drug.id = NEW.drug_id AND drug.freely_divisible) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KV002',
            MESSAGE = format('drug %s is freely divisible and takes no Teilmenge', NEW.drug_id),
            CONSTRAINT = 'drug_packaging_no_subset_when_freely_divisible';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_packaging_no_subset_when_freely_divisible
    BEFORE INSERT OR UPDATE OF kind, archived, drug_id ON drug_packaging
    FOR EACH ROW
    EXECUTE FUNCTION drug_packaging_no_subset_when_freely_divisible();

CREATE FUNCTION drug_freely_divisible_without_subsets() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.freely_divisible AND NOT OLD.freely_divisible
       AND EXISTS (SELECT 1 FROM drug_packaging subset
                    WHERE subset.drug_id = NEW.id
                      AND subset.kind = 'subset' AND NOT subset.archived) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KV002',
            MESSAGE = format('drug %s still has a Teilmenge', NEW.id),
            CONSTRAINT = 'drug_freely_divisible_without_subsets';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_freely_divisible_without_subsets
    BEFORE UPDATE OF freely_divisible ON drug
    FOR EACH ROW
    EXECUTE FUNCTION drug_freely_divisible_without_subsets();
