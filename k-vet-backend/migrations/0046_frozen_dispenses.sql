-- The positions of a released invoice stay editable (2026-11-22): the vet corrects a line after the
-- invoice went out, the appointment says "PDF veraltet", and "Rechnung aktualisieren" issues the
-- corrected invoice — as often as needed.
--
-- What makes that safe for the stock ledger (FR-020): a dispense is a *draft* until an invoice
-- with it is accepted, then *frozen* — and a frozen dispense is never deleted. Until now "frozen"
-- was the treatment's invoice status, which only worked while a released treatment could not be
-- edited. It becomes a mark on the dispense itself: a changed or removed line reverses its frozen
-- dispenses with linked corrections and books new drafts for what it holds now.
ALTER TABLE drug_stock_movement ADD COLUMN frozen_at TIMESTAMPTZ;

COMMENT ON COLUMN drug_stock_movement.frozen_at IS
    'When an accepted invoice froze this dispense; NULL while it is a draft. Corrections are history by nature and leave it empty.';

-- Frozen already: the dispenses of every released invoice, and every dispense that has been
-- reversed (a cancelled invoice, an update before this migration).
UPDATE drug_stock_movement movement
   SET frozen_at = now()
 WHERE movement.kind = 'dispense'
   AND (EXISTS (SELECT 1 FROM drug_stock_movement reversal
                 WHERE reversal.reverses_movement_id = movement.id)
        OR EXISTS (SELECT 1 FROM treatment_item item
                     JOIN invoice ON invoice.treatment_id = item.treatment_id
                    WHERE item.id = movement.treatment_item_id
                      AND invoice.status IN ('accepted', 'sent', 'submitted')));

-- A frozen dispense outlives its line: deleting the line keeps it, detached. Only then may a
-- dispense name no line.
ALTER TABLE drug_stock_movement
    DROP CONSTRAINT drug_stock_movement_dispense_shape,
    ADD CONSTRAINT drug_stock_movement_dispense_shape CHECK (
        kind <> 'dispense' OR (
            quantity < 0
            AND (treatment_item_id IS NOT NULL OR frozen_at IS NOT NULL)
            AND reason IS NULL
            AND reverses_movement_id IS NULL
        )
    );

-- Whatever deletes a line — the line itself, a removed animal, a deleted appointment — its frozen
-- dispenses go back on the shelf by linked corrections and stay in the ledger, detached; its
-- drafts go with it (the foreign key still cascades). Until now a line whose dispense had been
-- reversed could not be deleted at all: the reversal's reference stopped the cascade.
CREATE FUNCTION treatment_item_keeps_frozen_dispenses() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO drug_stock_movement (lot_id, kind, quantity, reason, reverses_movement_id)
    SELECT movement.lot_id, 'correction', -movement.quantity, 'Position entfernt', movement.id
      FROM drug_stock_movement movement
     WHERE movement.treatment_item_id = OLD.id
       AND movement.kind = 'dispense'
       AND movement.frozen_at IS NOT NULL
       AND NOT EXISTS (SELECT 1 FROM drug_stock_movement reversal
                        WHERE reversal.reverses_movement_id = movement.id);
    UPDATE drug_stock_movement
       SET treatment_item_id = NULL
     WHERE treatment_item_id = OLD.id
       AND frozen_at IS NOT NULL;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER treatment_item_keeps_frozen_dispenses
    BEFORE DELETE ON treatment_item
    FOR EACH ROW EXECUTE FUNCTION treatment_item_keeps_frozen_dispenses();
