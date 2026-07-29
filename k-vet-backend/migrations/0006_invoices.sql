-- Invoices and the per-scope invoice number counter (data-model.md "Invoicing").

CREATE TABLE invoice (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    treatment_id      BIGINT NOT NULL REFERENCES treatment (id),
    invoice_number    TEXT NOT NULL UNIQUE,
    invoice_date      DATE NOT NULL,
    status            invoice_status NOT NULL DEFAULT 'created',
    includes_finding  BOOLEAN NOT NULL DEFAULT false,
    note              TEXT,
    -- FK added in 0007 once attachment exists.
    pdf_attachment_id BIGINT,
    email_recipients  TEXT[],
    ts_accepted       TIMESTAMPTZ,
    ts_sent_email     TIMESTAMPTZ,
    ts_submitted      TIMESTAMPTZ,
    ts_cancelled      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT invoice_accepted_timestamp CHECK (status <> 'accepted' OR ts_accepted IS NOT NULL),
    CONSTRAINT invoice_submitted_timestamp CHECK (
        status <> 'submitted' OR (ts_accepted IS NOT NULL AND ts_submitted IS NOT NULL)
    ),
    CONSTRAINT invoice_cancelled_timestamp CHECK (
        status <> 'cancelled' OR ts_cancelled IS NOT NULL
    )
);

CREATE TRIGGER invoice_set_updated_at BEFORE UPDATE ON invoice
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- At most one live invoice per treatment; cancelled invoices are kept for bookkeeping.
CREATE UNIQUE INDEX invoice_one_live_per_treatment_idx
    ON invoice (treatment_id) WHERE status <> 'cancelled';

CREATE INDEX invoice_status_idx ON invoice (status);
CREATE INDEX invoice_date_idx ON invoice (invoice_date DESC, id DESC);

CREATE TABLE invoice_number_sequence (
    scope      TEXT PRIMARY KEY,
    counter    BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT invoice_number_sequence_counter_nonnegative CHECK (counter >= 0)
);

CREATE TRIGGER invoice_number_sequence_set_updated_at BEFORE UPDATE ON invoice_number_sequence
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
