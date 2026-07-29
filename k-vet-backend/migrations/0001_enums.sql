-- Enums, shared helpers and extensions (data-model.md "Enums").
--
-- Modeling conventions used by every following migration:
--   * identity primary key per entity
--   * created_at / updated_at TIMESTAMPTZ NOT NULL, updated_at maintained by trigger
--   * variants carry an explicit discriminator enum plus kind-bound CHECK constraints
--   * draft-enabled tables enforce their completeness set with `CHECK (draft OR (...))`
--
-- Optional column *groups* on draft-enabled tables (invoice address, second customer
-- name, supplier/manufacturer address) deliberately have no write-time all-or-none
-- CHECK: auto-save PATCHes single fields, so a half-typed group must be storable
-- (Constitution III). Their presence is instead defined once, in the database, by a
-- STORED generated boolean that every reader (invoice PDF, list views) uses — a
-- half-filled group can therefore never be rendered as an address.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TYPE salutation AS ENUM ('frau', 'herr', 'familie');
CREATE TYPE packaging_kind AS ENUM ('original', 'subset');
CREATE TYPE movement_kind AS ENUM ('dispense', 'correction');
CREATE TYPE service_type AS ENUM ('got', 'self_defined');
CREATE TYPE template_item_kind AS ENUM ('drug_packaging', 'service');
CREATE TYPE treatment_item_kind AS ENUM ('drug_packaging', 'service');
CREATE TYPE attachment_kind AS ENUM ('patient_file', 'treatment_file', 'referenced');
CREATE TYPE invoice_status AS ENUM ('created', 'accepted', 'submitted', 'cancelled');
CREATE TYPE email_type AS ENUM ('private', 'work', 'other');

-- Keeps updated_at honest without the application having to remember it.
CREATE FUNCTION set_updated_at() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;
