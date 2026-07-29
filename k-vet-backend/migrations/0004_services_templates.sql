-- Services (GOT and self-defined) and treatment templates
-- (data-model.md "Services & templates").

CREATE TABLE service (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    type            service_type NOT NULL,
    name            TEXT,
    got_number      TEXT,
    factor          NUMERIC(7, 3) DEFAULT 100,
    vat_percent     NUMERIC(7, 3),
    gross_price     NUMERIC(10, 2),
    travel_expenses BOOLEAN NOT NULL DEFAULT false,
    hidden          BOOLEAN NOT NULL DEFAULT false,
    archived        BOOLEAN NOT NULL DEFAULT false,
    draft           BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT service_complete CHECK (
        draft OR (
            name IS NOT NULL AND vat_percent IS NOT NULL AND gross_price IS NOT NULL
        )
    ),
    -- GOT positions carry number and factor; self-defined services never carry a GOT number.
    CONSTRAINT service_got_shape CHECK (
        draft OR (
            (type = 'got' AND got_number IS NOT NULL AND factor IS NOT NULL)
            OR (type = 'self_defined' AND got_number IS NULL)
        )
    ),
    CONSTRAINT service_factor_positive CHECK (factor IS NULL OR factor > 0),
    CONSTRAINT service_vat_percent_range CHECK (vat_percent IS NULL OR vat_percent >= 0)
);

CREATE TRIGGER service_set_updated_at BEFORE UPDATE ON service
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX service_name_trgm_idx ON service USING gin (name gin_trgm_ops);
CREATE INDEX service_got_number_idx ON service (got_number) WHERE got_number IS NOT NULL;

CREATE TABLE treatment_template (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT,
    archived   BOOLEAN NOT NULL DEFAULT false,
    draft      BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT treatment_template_complete CHECK (draft OR name IS NOT NULL)
);

CREATE TRIGGER treatment_template_set_updated_at BEFORE UPDATE ON treatment_template
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE treatment_template_item (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    template_id       BIGINT NOT NULL REFERENCES treatment_template (id) ON DELETE CASCADE,
    position          INTEGER NOT NULL,
    kind              template_item_kind NOT NULL,
    drug_packaging_id BIGINT REFERENCES drug_packaging (id),
    service_id        BIGINT REFERENCES service (id),
    quantity          NUMERIC(10, 2) NOT NULL,
    unit              TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT treatment_template_item_quantity_positive CHECK (quantity > 0),
    CONSTRAINT treatment_template_item_xor CHECK (
        (kind = 'drug_packaging') = (drug_packaging_id IS NOT NULL)
        AND (kind = 'service') = (service_id IS NOT NULL)
    ),
    -- Deferred so reorder operations can shuffle positions inside one transaction.
    CONSTRAINT treatment_template_item_position_key UNIQUE (template_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TRIGGER treatment_template_item_set_updated_at BEFORE UPDATE ON treatment_template_item
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX treatment_template_item_template_idx
    ON treatment_template_item (template_id, position);
