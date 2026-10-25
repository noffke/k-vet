-- Preisauskünfte, kept.
--
-- A quote for a caller is saved as it is put together, like every other record. It is a draft
-- until it belongs to a customer; then it completes itself and is listed on their page, to be
-- opened again or taken into a visit. A draft — a call nobody attached to a customer — stays
-- reachable under "Offene Preisauskünfte" and is cleaned up 28 days after its last change.
--
-- The lines keep what they were quoted at: name, net price and VAT of their last save. Taken
-- into a visit, they are billed at those prices; where the catalogue has moved since, the
-- application says so.
CREATE TABLE price_quote (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    customer_id    BIGINT REFERENCES customer (id),
    name           TEXT,
    note           TEXT,
    -- A new caller's address, for the Wegegeld, until they are a customer.
    address_street TEXT,
    address_zip    TEXT,
    address_city   TEXT,
    draft          BOOLEAN NOT NULL DEFAULT true,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT price_quote_complete CHECK (draft OR customer_id IS NOT NULL)
);

CREATE TRIGGER price_quote_set_updated_at BEFORE UPDATE ON price_quote
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX price_quote_customer_idx ON price_quote (customer_id, created_at DESC)
    WHERE NOT draft;
CREATE INDEX price_quote_open_idx ON price_quote (updated_at DESC) WHERE draft;

CREATE TABLE price_quote_line (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    quote_id          BIGINT NOT NULL REFERENCES price_quote (id) ON DELETE CASCADE,
    position          INTEGER NOT NULL,
    kind              treatment_item_kind NOT NULL,
    drug_packaging_id BIGINT REFERENCES drug_packaging (id),
    service_id        BIGINT REFERENCES service (id),
    quantity          NUMERIC(10, 2) NOT NULL,
    factor            NUMERIC(7, 3),
    km                NUMERIC(10, 2),
    km_multiplier     NUMERIC(7, 3),
    -- As quoted: what the line was priced at when it was last saved.
    name              TEXT NOT NULL,
    price_net         NUMERIC(12, 2) NOT NULL,
    vat_percent       NUMERIC(7, 3) NOT NULL,
    CONSTRAINT price_quote_line_reference CHECK (
        (kind = 'drug_packaging') = (drug_packaging_id IS NOT NULL)
        AND (kind = 'service') = (service_id IS NOT NULL)
    ),
    CONSTRAINT price_quote_line_quantity_positive CHECK (quantity > 0),
    CONSTRAINT price_quote_line_factor_positive CHECK (factor IS NULL OR factor > 0),
    CONSTRAINT price_quote_line_km_not_negative CHECK (km IS NULL OR km >= 0),
    CONSTRAINT price_quote_line_multiplier_positive CHECK (km_multiplier IS NULL OR km_multiplier > 0),
    CONSTRAINT price_quote_line_position_key UNIQUE (quote_id, position)
);
