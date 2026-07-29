-- Drugs, packagings, stock lots and the append-only movement ledger
-- (data-model.md "Pharmacy").

CREATE TABLE drug (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name               TEXT,
    manufacturer_id    BIGINT REFERENCES manufacturer (id),
    submission_receipt BOOLEAN NOT NULL DEFAULT false,
    narcotic           BOOLEAN NOT NULL DEFAULT false,
    vaccine            BOOLEAN NOT NULL DEFAULT false,
    refrigerate        BOOLEAN NOT NULL DEFAULT false,
    redesignation      BOOLEAN NOT NULL DEFAULT false,
    vat_percent        NUMERIC(7, 3),
    approval_number    TEXT,
    archived           BOOLEAN NOT NULL DEFAULT false,
    draft              BOOLEAN NOT NULL DEFAULT true,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drug_complete CHECK (
        draft OR (
            name IS NOT NULL AND manufacturer_id IS NOT NULL AND vat_percent IS NOT NULL
        )
    ),
    CONSTRAINT drug_vat_percent_range CHECK (vat_percent IS NULL OR vat_percent >= 0)
);

CREATE TRIGGER drug_set_updated_at BEFORE UPDATE ON drug
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX drug_name_trgm_idx ON drug USING gin (name gin_trgm_ops);
CREATE INDEX drug_manufacturer_idx ON drug (manufacturer_id);

CREATE TABLE drug_packaging (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    drug_id           BIGINT NOT NULL REFERENCES drug (id),
    kind              packaging_kind NOT NULL,
    unit              TEXT,
    quantity          NUMERIC(10, 2),
    list_price_net    NUMERIC(10, 2),
    sales_price_gross NUMERIC(10, 2),
    price_overridden  BOOLEAN NOT NULL DEFAULT false,
    supplier_id       BIGINT REFERENCES supplier (id),
    archived          BOOLEAN NOT NULL DEFAULT false,
    draft             BOOLEAN NOT NULL DEFAULT true,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drug_packaging_complete CHECK (
        draft OR (
            unit IS NOT NULL
            AND quantity IS NOT NULL
            AND list_price_net IS NOT NULL
            AND sales_price_gross IS NOT NULL
        )
    ),
    CONSTRAINT drug_packaging_quantity_positive CHECK (quantity IS NULL OR quantity > 0),
    -- Only original packagings are bought from a supplier — checked once the row is complete.
    CONSTRAINT drug_packaging_supplier_kind CHECK (
        draft OR ((kind = 'original') = (supplier_id IS NOT NULL))
    ),
    -- Target of the composite foreign key from drug_stock_lot.
    CONSTRAINT drug_packaging_id_kind_key UNIQUE (id, kind)
);

CREATE TRIGGER drug_packaging_set_updated_at BEFORE UPDATE ON drug_packaging
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- At most one original packaging per drug (drafts occupy the slot as well).
CREATE UNIQUE INDEX drug_packaging_one_original_idx
    ON drug_packaging (drug_id) WHERE kind = 'original';

CREATE INDEX drug_packaging_drug_idx ON drug_packaging (drug_id);

CREATE TABLE drug_stock_lot (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    packaging_id      BIGINT NOT NULL,
    packaging_kind    packaging_kind NOT NULL,
    arrival_date      DATE NOT NULL DEFAULT current_date,
    packages_received INTEGER NOT NULL,
    initial_quantity  NUMERIC(10, 2) NOT NULL,
    batch_number      TEXT,
    expiration_date   DATE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drug_stock_lot_packages_positive CHECK (packages_received > 0),
    CONSTRAINT drug_stock_lot_initial_quantity_positive CHECK (initial_quantity > 0),
    -- Stock exists for original packagings only — enforced in the database, not in code.
    CONSTRAINT drug_stock_lot_original_only CHECK (packaging_kind = 'original'),
    CONSTRAINT drug_stock_lot_packaging_fkey FOREIGN KEY (packaging_id, packaging_kind)
        REFERENCES drug_packaging (id, kind)
);

CREATE TRIGGER drug_stock_lot_set_updated_at BEFORE UPDATE ON drug_stock_lot
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX drug_stock_lot_packaging_idx ON drug_stock_lot (packaging_id);
-- FEFO suggestion and the dashboard expiry widget only ever look at dated lots.
CREATE INDEX drug_stock_lot_expiration_idx ON drug_stock_lot (expiration_date)
    WHERE expiration_date IS NOT NULL;

CREATE TABLE drug_stock_movement (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    lot_id               BIGINT NOT NULL REFERENCES drug_stock_lot (id),
    kind                 movement_kind NOT NULL,
    quantity             NUMERIC(10, 2) NOT NULL,
    moved_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- FK added in 0005 once treatment_item exists.
    treatment_item_id    BIGINT,
    reason               TEXT,
    reverses_movement_id BIGINT REFERENCES drug_stock_movement (id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drug_stock_movement_quantity_nonzero CHECK (quantity <> 0),
    CONSTRAINT drug_stock_movement_dispense_shape CHECK (
        kind <> 'dispense' OR (
            quantity < 0
            AND treatment_item_id IS NOT NULL
            AND reason IS NULL
            AND reverses_movement_id IS NULL
        )
    ),
    CONSTRAINT drug_stock_movement_correction_shape CHECK (
        kind <> 'correction' OR treatment_item_id IS NULL
    )
);

CREATE TRIGGER drug_stock_movement_set_updated_at BEFORE UPDATE ON drug_stock_movement
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX drug_stock_movement_lot_idx ON drug_stock_movement (lot_id, moved_at, id);
CREATE INDEX drug_stock_movement_treatment_item_idx ON drug_stock_movement (treatment_item_id)
    WHERE treatment_item_id IS NOT NULL;

-- Remaining stock is always derived, never stored (FR-018).
CREATE VIEW lot_remaining AS
SELECT lot.id                AS lot_id,
       lot.packaging_id      AS packaging_id,
       packaging.drug_id     AS drug_id,
       lot.batch_number      AS batch_number,
       lot.expiration_date   AS expiration_date,
       lot.arrival_date      AS arrival_date,
       lot.initial_quantity
           + COALESCE(SUM(movement.quantity), 0) AS remaining
FROM drug_stock_lot lot
JOIN drug_packaging packaging ON packaging.id = lot.packaging_id
LEFT JOIN drug_stock_movement movement ON movement.lot_id = lot.id
GROUP BY lot.id, lot.packaging_id, packaging.drug_id, lot.batch_number,
         lot.expiration_date, lot.arrival_date, lot.initial_quantity;
