-- Customers, their email addresses, patients, suppliers and manufacturers
-- (data-model.md "Master data").

CREATE TABLE customer (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    salutation         salutation,
    first_name         TEXT,
    last_name          TEXT,
    second_salutation  salutation,
    second_first_name  TEXT,
    second_last_name   TEXT,
    home_addon         TEXT,
    home_street        TEXT,
    home_zip           TEXT,
    home_city          TEXT,
    invoice_salutation salutation,
    invoice_first_name TEXT,
    invoice_last_name  TEXT,
    invoice_addon      TEXT,
    invoice_street     TEXT,
    invoice_zip        TEXT,
    invoice_city       TEXT,
    phone              TEXT,
    warning_remark     TEXT,
    archived           BOOLEAN NOT NULL DEFAULT false,
    draft              BOOLEAN NOT NULL DEFAULT true,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A second name is addressed on the invoice only when salutation and last name are known.
    has_second_name    BOOLEAN NOT NULL GENERATED ALWAYS AS (
                           second_salutation IS NOT NULL AND second_last_name IS NOT NULL
                       ) STORED,
    -- The invoice address replaces the home address only when it is complete.
    has_invoice_address BOOLEAN NOT NULL GENERATED ALWAYS AS (
                           invoice_salutation IS NOT NULL
                           AND invoice_last_name IS NOT NULL
                           AND invoice_street IS NOT NULL
                           AND invoice_zip IS NOT NULL
                           AND invoice_city IS NOT NULL
                       ) STORED,
    CONSTRAINT customer_complete CHECK (
        draft OR (
            salutation IS NOT NULL
            AND last_name IS NOT NULL
            AND home_street IS NOT NULL
            AND home_zip IS NOT NULL
            AND home_city IS NOT NULL
        )
    )
);

CREATE TRIGGER customer_set_updated_at BEFORE UPDATE ON customer
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX customer_name_idx ON customer (last_name, first_name);
CREATE INDEX customer_last_name_trgm_idx ON customer USING gin (last_name gin_trgm_ops);
CREATE INDEX customer_first_name_trgm_idx ON customer USING gin (first_name gin_trgm_ops);

CREATE TABLE customer_email (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customer (id) ON DELETE CASCADE,
    email       TEXT NOT NULL,
    email_type  email_type NOT NULL DEFAULT 'private',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER customer_email_set_updated_at BEFORE UPDATE ON customer_email
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX customer_email_customer_idx ON customer_email (customer_id);

CREATE TABLE patient (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    customer_id         BIGINT REFERENCES customer (id),
    name                TEXT,
    sex                 TEXT,
    species             TEXT,
    race                TEXT,
    colour              TEXT,
    date_of_birth       DATE,
    photo_attachment_id BIGINT,
    date_of_death       DATE,
    chip_number         TEXT,
    eu_passport_number  TEXT,
    warning_remark      TEXT,
    neutered            BOOLEAN NOT NULL DEFAULT false,
    insured             BOOLEAN NOT NULL DEFAULT false,
    archived            BOOLEAN NOT NULL DEFAULT false,
    draft               BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT patient_complete CHECK (
        draft OR (
            customer_id IS NOT NULL
            AND name IS NOT NULL
            AND sex IS NOT NULL
            AND species IS NOT NULL
        )
    ),
    CONSTRAINT patient_sex_values CHECK (sex IS NULL OR sex IN ('female', 'male', 'unknown'))
);

CREATE TRIGGER patient_set_updated_at BEFORE UPDATE ON patient
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX patient_customer_idx ON patient (customer_id);
CREATE INDEX patient_name_trgm_idx ON patient USING gin (name gin_trgm_ops);

CREATE TABLE supplier (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT,
    addr_addon  TEXT,
    addr_street TEXT,
    addr_zip    TEXT,
    addr_city   TEXT,
    archived    BOOLEAN NOT NULL DEFAULT false,
    draft       BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    has_address BOOLEAN NOT NULL GENERATED ALWAYS AS (
                    addr_street IS NOT NULL AND addr_zip IS NOT NULL AND addr_city IS NOT NULL
                ) STORED,
    CONSTRAINT supplier_complete CHECK (draft OR name IS NOT NULL)
);

CREATE TRIGGER supplier_set_updated_at BEFORE UPDATE ON supplier
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE manufacturer (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT,
    addr_addon  TEXT,
    addr_street TEXT,
    addr_zip    TEXT,
    addr_city   TEXT,
    archived    BOOLEAN NOT NULL DEFAULT false,
    draft       BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    has_address BOOLEAN NOT NULL GENERATED ALWAYS AS (
                    addr_street IS NOT NULL AND addr_zip IS NOT NULL AND addr_city IS NOT NULL
                ) STORED,
    CONSTRAINT manufacturer_complete CHECK (draft OR name IS NOT NULL)
);

CREATE TRIGGER manufacturer_set_updated_at BEFORE UPDATE ON manufacturer
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
