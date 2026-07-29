-- Attachments, practice settings, picker usage weights and the session store
-- (data-model.md "Files & settings").

CREATE TABLE attachment (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    sha256         TEXT NOT NULL,
    mime_type      TEXT NOT NULL,
    size_bytes     BIGINT NOT NULL,
    orig_name      TEXT NOT NULL,
    kind           attachment_kind NOT NULL,
    patient_id     BIGINT REFERENCES patient (id) ON DELETE CASCADE,
    treatment_id   BIGINT REFERENCES treatment (id) ON DELETE CASCADE,
    reference_date DATE,
    note           TEXT,
    has_thumbnail  BOOLEAN NOT NULL DEFAULT false,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT attachment_size_nonnegative CHECK (size_bytes >= 0),
    CONSTRAINT attachment_sha256_hex CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT attachment_patient_file_shape CHECK (
        kind <> 'patient_file' OR (patient_id IS NOT NULL AND treatment_id IS NULL)
    ),
    CONSTRAINT attachment_treatment_file_shape CHECK (
        kind <> 'treatment_file' OR (treatment_id IS NOT NULL AND patient_id IS NULL)
    ),
    -- `referenced` attachments are pointed at by their owner (patient photo, invoice PDF, logo).
    CONSTRAINT attachment_referenced_shape CHECK (
        kind <> 'referenced' OR (patient_id IS NULL AND treatment_id IS NULL)
    )
);

CREATE TRIGGER attachment_set_updated_at BEFORE UPDATE ON attachment
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX attachment_sha256_idx ON attachment (sha256);
CREATE INDEX attachment_patient_idx ON attachment (patient_id) WHERE patient_id IS NOT NULL;
CREATE INDEX attachment_treatment_idx ON attachment (treatment_id) WHERE treatment_id IS NOT NULL;

ALTER TABLE patient
    ADD CONSTRAINT patient_photo_attachment_fkey
    FOREIGN KEY (photo_attachment_id) REFERENCES attachment (id) ON DELETE SET NULL;

ALTER TABLE invoice
    ADD CONSTRAINT invoice_pdf_attachment_fkey
    FOREIGN KEY (pdf_attachment_id) REFERENCES attachment (id) ON DELETE SET NULL;

CREATE TABLE global_settings (
    id                  BOOLEAN PRIMARY KEY DEFAULT true,
    practice_name       TEXT NOT NULL DEFAULT '',
    practice_address    TEXT NOT NULL DEFAULT '',
    iban                TEXT NOT NULL DEFAULT '',
    ustid               TEXT NOT NULL DEFAULT '',
    logo_attachment_id  BIGINT REFERENCES attachment (id) ON DELETE SET NULL,
    cc_emails           TEXT[] NOT NULL DEFAULT '{}',
    bcc_emails          TEXT[] NOT NULL DEFAULT '{}',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The classic single-row guard.
    CONSTRAINT global_settings_single_row CHECK (id)
);

CREATE TRIGGER global_settings_set_updated_at BEFORE UPDATE ON global_settings
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

INSERT INTO global_settings (id) VALUES (true);

-- Usage weights for the unified picker, rebuilt nightly from treatment_item counts.
CREATE TABLE picker_usage (
    kind       treatment_item_kind NOT NULL,
    item_id    BIGINT NOT NULL,
    uses       BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kind, item_id),
    CONSTRAINT picker_usage_uses_nonnegative CHECK (uses >= 0)
);

CREATE TRIGGER picker_usage_set_updated_at BEFORE UPDATE ON picker_usage
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Session store for tower-sessions (tower-sessions-sqlx-store layout). Created here so
-- `#[sqlx::test]` databases have it; the store's own migrate() call at boot is idempotent.
CREATE SCHEMA IF NOT EXISTS tower_sessions;

CREATE TABLE IF NOT EXISTS tower_sessions.session (
    id          TEXT PRIMARY KEY NOT NULL,
    data        BYTEA NOT NULL,
    expiry_date TIMESTAMPTZ NOT NULL
);
