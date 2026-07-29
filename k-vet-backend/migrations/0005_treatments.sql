-- Appointments, treatments and their billing lines
-- (data-model.md "Appointments & treatments").

CREATE TABLE appointment (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    starts_at  TIMESTAMPTZ,
    note       TEXT,
    draft      BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT appointment_complete CHECK (draft OR starts_at IS NOT NULL)
);

CREATE TRIGGER appointment_set_updated_at BEFORE UPDATE ON appointment
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX appointment_starts_at_idx ON appointment (starts_at DESC);

CREATE TABLE treatment (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    appointment_id   BIGINT NOT NULL REFERENCES appointment (id) ON DELETE CASCADE,
    treatment_reason TEXT,
    finding          TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER treatment_set_updated_at BEFORE UPDATE ON treatment
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX treatment_appointment_idx ON treatment (appointment_id);

CREATE TABLE treatment_patient (
    treatment_id BIGINT NOT NULL REFERENCES treatment (id) ON DELETE CASCADE,
    patient_id   BIGINT NOT NULL REFERENCES patient (id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (treatment_id, patient_id)
);

CREATE INDEX treatment_patient_patient_idx ON treatment_patient (patient_id);

CREATE TABLE treatment_item (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    treatment_id      BIGINT NOT NULL REFERENCES treatment (id) ON DELETE CASCADE,
    position          INTEGER NOT NULL,
    kind              treatment_item_kind NOT NULL,
    drug_packaging_id BIGINT REFERENCES drug_packaging (id),
    service_id        BIGINT REFERENCES service (id),
    patient_id        BIGINT REFERENCES patient (id),
    -- Everything below is pinned at line entry and never re-read from the catalog.
    name              TEXT NOT NULL,
    quantity          NUMERIC(10, 2) NOT NULL,
    unit              TEXT,
    factor            NUMERIC(7, 3),
    got_number        TEXT,
    price_gross       NUMERIC(10, 2) NOT NULL,
    vat_percent       NUMERIC(7, 3) NOT NULL,
    km                NUMERIC(10, 2),
    km_multiplier     NUMERIC(7, 3),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT treatment_item_quantity_positive CHECK (quantity > 0),
    CONSTRAINT treatment_item_xor CHECK (
        (kind = 'drug_packaging') = (drug_packaging_id IS NOT NULL)
        AND (kind = 'service') = (service_id IS NOT NULL)
    ),
    -- Drug lines are always attributed to a patient; service lines may stay unattributed.
    CONSTRAINT treatment_item_drug_needs_patient CHECK (
        kind <> 'drug_packaging' OR patient_id IS NOT NULL
    ),
    -- Drug lines carry a unit and none of the service-only columns.
    CONSTRAINT treatment_item_drug_shape CHECK (
        kind <> 'drug_packaging' OR (
            unit IS NOT NULL
            AND factor IS NULL
            AND got_number IS NULL
            AND km IS NULL
            AND km_multiplier IS NULL
        )
    ),
    CONSTRAINT treatment_item_km_shape CHECK (
        (km IS NULL AND km_multiplier IS NULL) OR (km >= 0 AND km_multiplier > 0)
    ),
    CONSTRAINT treatment_item_vat_percent_range CHECK (vat_percent >= 0),
    -- Deferred so reorder operations can shuffle positions inside one transaction.
    CONSTRAINT treatment_item_position_key UNIQUE (treatment_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TRIGGER treatment_item_set_updated_at BEFORE UPDATE ON treatment_item
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX treatment_item_treatment_idx ON treatment_item (treatment_id, position);
CREATE INDEX treatment_item_packaging_idx ON treatment_item (drug_packaging_id)
    WHERE drug_packaging_id IS NOT NULL;
CREATE INDEX treatment_item_service_idx ON treatment_item (service_id)
    WHERE service_id IS NOT NULL;

-- Dispense movements point at the line that caused them (declared in 0003 without the FK).
ALTER TABLE drug_stock_movement
    ADD CONSTRAINT drug_stock_movement_treatment_item_fkey
    FOREIGN KEY (treatment_item_id) REFERENCES treatment_item (id) ON DELETE CASCADE;
