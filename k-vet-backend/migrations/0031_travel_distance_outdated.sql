-- Wegegeld kilometres follow the address they were driven to.
--
-- A new Wegegeld line takes the kilometres of the customer's previous visit, so a correction
-- the vet made carries forward. Once the customer's home address — or the practice's own — has
-- changed, those kilometres no longer describe the way there: the customer is marked, and the
-- next new line asks Google again instead. Lines and invoices already written keep their
-- kilometres. The application clears the mark when a new distance is in, from Google or typed.
ALTER TABLE customer ADD COLUMN travel_distance_outdated BOOLEAN NOT NULL DEFAULT false;

CREATE FUNCTION customer_mark_travel_distance() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.home_street, NEW.home_zip, NEW.home_city, NEW.home_country)
       IS DISTINCT FROM (OLD.home_street, OLD.home_zip, OLD.home_city, OLD.home_country) THEN
        NEW.travel_distance_outdated := true;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER customer_mark_travel_distance
    BEFORE UPDATE OF home_street, home_zip, home_city, home_country ON customer
    FOR EACH ROW EXECUTE FUNCTION customer_mark_travel_distance();

-- The practice moving changes every way there at once.
CREATE FUNCTION global_settings_mark_travel_distance() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE customer SET travel_distance_outdated = true WHERE NOT travel_distance_outdated;
    RETURN NULL;
END
$$;

CREATE TRIGGER global_settings_mark_travel_distance
    AFTER UPDATE OF practice_street, practice_zip, practice_city, practice_country
    ON global_settings
    FOR EACH ROW
    WHEN ((NEW.practice_street, NEW.practice_zip, NEW.practice_city, NEW.practice_country)
          IS DISTINCT FROM
          (OLD.practice_street, OLD.practice_zip, OLD.practice_city, OLD.practice_country))
    EXECUTE FUNCTION global_settings_mark_travel_distance();
