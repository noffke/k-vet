-- Textbausteine: named snippets the vet assembles a Vorbericht or a Therapie from.
--
-- Findings and treatment reasons repeat across visits almost verbatim ("Impfung nach Schema,
-- Tier zeigte keine Auffälligkeiten"), and typing them out on a phone during a house call is
-- the slowest part of the record. A block is just a name and its text; where it was used is
-- deliberately not recorded — once inserted the text belongs to that patient's record and is
-- edited there, so a link back would only invite the illusion that changing the block changes
-- what was already written.
--
-- Draft rows and archiving follow the master-data conventions (datamodel.md): the completeness
-- set is enforced as `draft OR (…)` rather than plain NOT NULL, so auto-save has a row to write
-- against from the first keystroke.

CREATE TABLE text_block (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT,
    content    TEXT,
    archived   BOOLEAN NOT NULL DEFAULT false,
    draft      BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT text_block_complete CHECK (
        draft OR (name IS NOT NULL AND content IS NOT NULL)
    )
);

CREATE TRIGGER text_block_set_updated_at BEFORE UPDATE ON text_block
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- The picker searches name and content alike; both get a trigram index so a fuzzy match over
-- the library stays cheap as it grows.
CREATE INDEX text_block_name_trgm_idx    ON text_block USING gin (name gin_trgm_ops);
CREATE INDEX text_block_content_trgm_idx ON text_block USING gin (content gin_trgm_ops);
