-- "Mitbringen": a drug the vet is asked to bring to a visit — a customer phones for more of a
-- regular medication, and it used to go on a slip of paper. A reminder, not yet a billed line:
-- nothing is dispensed and no animal is named. On the day it becomes a position with one
-- click, and from then on it points at that line.
CREATE TABLE appointment_reminder (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    appointment_id    BIGINT NOT NULL REFERENCES appointment (id) ON DELETE CASCADE,
    drug_packaging_id BIGINT NOT NULL REFERENCES drug_packaging (id),
    -- Packagings, as a line counts them.
    quantity          NUMERIC(10, 2) NOT NULL,
    note              TEXT,
    -- Ticked off on the Packliste.
    packed            BOOLEAN NOT NULL DEFAULT false,
    position          INTEGER NOT NULL,
    -- The position it became. A line removed again makes it a reminder again.
    taken_item_id     BIGINT UNIQUE REFERENCES treatment_item (id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT appointment_reminder_quantity_positive CHECK (quantity > 0),
    -- Deferred like the lines' positions, so a reorder or a gap closed can pass collisions.
    CONSTRAINT appointment_reminder_position_key UNIQUE (appointment_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TRIGGER appointment_reminder_set_updated_at BEFORE UPDATE ON appointment_reminder
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX appointment_reminder_packaging_idx ON appointment_reminder (drug_packaging_id);
