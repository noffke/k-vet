-- "Ohne Bestand": consumables — syringes, needles, bandages — put on the invoice like a drug
-- but without any stock kept for them (issues.md, decided 2026-11-18).
--
-- Such a drug has no lots, so nothing is ever taken off a shelf and no dispense is refused for
-- want of stock. It is billed in whole packs — no Teilmenge, not "Beliebig teilbar" — and it is
-- not priced by the AMPreisV: its sales price is the one the vet types (`drugs::recompute_prices`
-- leaves it alone). The rules span tables, so triggers hold them, as in migration 0041; the
-- constraint names are what the API turns into field errors (`error.rs`).

ALTER TABLE drug ADD COLUMN without_stock BOOLEAN NOT NULL DEFAULT false;

-- Both on the same row, so a CHECK: "Beliebig teilbar" prices a rest as its share of the pack
-- with the Teilmenge mark-up, which a consumable does not get.
ALTER TABLE drug ADD CONSTRAINT drug_without_stock_not_divisible
    CHECK (NOT (without_stock AND freely_divisible));

-- Setting the flag: not while stock is left on the shelf (correct it to 0 first, or it would
-- be hidden for good), and not while the drug still has a Teilmenge. An archived Teilmenge —
-- one a billed line still names, deleted from the drug's page — does not count.
CREATE FUNCTION drug_without_stock_check() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.without_stock AND NOT OLD.without_stock THEN
        IF EXISTS (SELECT 1 FROM lot_remaining
                    WHERE lot_remaining.drug_id = NEW.id AND lot_remaining.remaining > 0) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'KV002',
                MESSAGE = format('drug %s still has stock', NEW.id),
                CONSTRAINT = 'drug_without_stock_has_stock';
        END IF;
        IF EXISTS (SELECT 1 FROM drug_packaging subset
                    WHERE subset.drug_id = NEW.id
                      AND subset.kind = 'subset' AND NOT subset.archived) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'KV002',
                MESSAGE = format('drug %s still has a Teilmenge', NEW.id),
                CONSTRAINT = 'drug_without_stock_without_subsets';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_without_stock_check
    BEFORE UPDATE OF without_stock ON drug
    FOR EACH ROW
    EXECUTE FUNCTION drug_without_stock_check();

-- While it is set, no Teilmenge is added.
CREATE FUNCTION drug_packaging_no_subset_without_stock() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.kind = 'subset' AND NOT NEW.archived
       AND EXISTS (SELECT 1 FROM drug WHERE drug.id = NEW.drug_id AND drug.without_stock) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KV002',
            MESSAGE = format('drug %s is kept without stock and takes no Teilmenge', NEW.drug_id),
            CONSTRAINT = 'drug_packaging_no_subset_without_stock';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_packaging_no_subset_without_stock
    BEFORE INSERT OR UPDATE OF kind, archived, drug_id ON drug_packaging
    FOR EACH ROW
    EXECUTE FUNCTION drug_packaging_no_subset_without_stock();

-- While it is set, no lot arrives (no Wareneingang) ...
CREATE FUNCTION drug_stock_lot_tracked() RETURNS TRIGGER AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM drug_packaging packaging
                 JOIN drug ON drug.id = packaging.drug_id
                WHERE packaging.id = NEW.packaging_id AND drug.without_stock) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KV002',
            MESSAGE = format('packaging %s belongs to a drug kept without stock', NEW.packaging_id),
            CONSTRAINT = 'drug_stock_lot_tracked';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_stock_lot_tracked
    BEFORE INSERT ON drug_stock_lot
    FOR EACH ROW
    EXECUTE FUNCTION drug_stock_lot_tracked();

-- ... and nothing moves on a lot left from before the flag: no stock correction, no dispense
-- from it. A reversal still may — cancelling an invoice billed before the flag puts its
-- dispense back, which is the ledger staying true, not stock being kept.
CREATE FUNCTION drug_stock_movement_tracked() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.reverses_movement_id IS NULL
       AND EXISTS (SELECT 1 FROM drug_stock_lot lot
                     JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
                     JOIN drug ON drug.id = packaging.drug_id
                    WHERE lot.id = NEW.lot_id AND drug.without_stock) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KV002',
            MESSAGE = format('lot %s belongs to a drug kept without stock', NEW.lot_id),
            CONSTRAINT = 'drug_stock_movement_tracked';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drug_stock_movement_tracked
    BEFORE INSERT ON drug_stock_movement
    FOR EACH ROW
    EXECUTE FUNCTION drug_stock_movement_tracked();
