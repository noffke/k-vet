-- A lot's remaining stock must never go below zero (FR-018, issues.md 8).
--
-- Remaining stock is derived, never stored (`lot_remaining`), and a view cannot carry a CHECK —
-- so the invariant lives in a trigger over the ledger instead. It is the one place that sees
-- every path: the stocktake correction, the FEFO dispense, an explicit lot choice, the
-- compensating rows written when an invoice is cancelled, and anything typed into psql.
--
-- Until now only the individual movement was constrained (non-zero, correct sign and shape);
-- nothing looked at the running sum. A dispense the lots could not cover was deliberately
-- booked in full and the lot went negative, on the reasoning that the shelf is the truth. A
-- negative remainder is not the shelf, though — it is a record that the books were already
-- wrong, and it spreads quietly, because FEFO keeps offering lots that hold nothing. The
-- remedy is the stocktake that already exists.

CREATE FUNCTION drug_stock_movement_keeps_lot_non_negative() RETURNS TRIGGER AS $$
DECLARE
    lot BIGINT := COALESCE(NEW.lot_id, OLD.lot_id);
    left_over NUMERIC;
BEGIN
    SELECT remaining INTO left_over FROM lot_remaining WHERE lot_id = lot;

    IF left_over < 0 THEN
        -- A distinct SQLSTATE so the API can tell this from any other constraint and say how
        -- much is short rather than "something went wrong".
        RAISE EXCEPTION USING
            ERRCODE = 'KV001',
            MESSAGE = format('lot %s would be left at %s', lot, left_over),
            CONSTRAINT = 'drug_stock_movement_lot_not_negative';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- AFTER, because the sum is only knowable once the row is in. A constraint trigger so it can
-- be deferred where a legitimate sequence dips below zero in the middle — rewriting a draft
-- dispense deletes and re-inserts, and the deletes are what land first.
CREATE CONSTRAINT TRIGGER drug_stock_movement_lot_not_negative
    AFTER INSERT OR UPDATE OR DELETE ON drug_stock_movement
    DEFERRABLE INITIALLY IMMEDIATE
    FOR EACH ROW
    EXECUTE FUNCTION drug_stock_movement_keeps_lot_non_negative();
