-- Formatted text: the two texts of a Patientenbehandlung (Vorbericht und Untersuchung, Therapie
-- und weiteres Vorgehen) and the text blocks inserted into them carry bold, italic, underline and
-- one level of bullet or numbered list.
--
-- They are stored as the editor's own document (ProseMirror JSON, the shape
-- `domain::rich_text::RichText` reads and writes) rather than as Markdown: the editor and the
-- invoice PDF then read one structure, there is no second parser to agree with the first, and
-- nothing the vet types can turn into formatting by accident when it is read back.
--
--   {"type": "doc", "content": [
--      {"type": "paragraph", "content": [
--         {"type": "text", "text": "Ohr ", "marks": [{"type": "bold"}]},
--         {"type": "hardBreak"}]},
--      {"type": "orderedList", "attrs": {"start": 1}, "content": [
--         {"type": "listItem", "content": [{"type": "paragraph", "content": [...]}]}]}]}
--
-- A field the vet emptied is NULL, never a document without text: `has_content` (the customer
-- lock) and the draft rules of text blocks count IS NOT NULL.

-- Plain text into a document, one paragraph per line. Kept rather than dropped after this
-- migration: it is how anything that only has plain text (an import, a hand-written fix) writes
-- these columns.
CREATE FUNCTION rich_text_from_plain(plain TEXT) RETURNS JSONB
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT CASE WHEN plain ~ '\S' THEN jsonb_build_object(
        'type', 'doc',
        'content', (
            SELECT jsonb_agg(
                       CASE WHEN line ~ '\S'
                            THEN jsonb_build_object(
                                     'type', 'paragraph',
                                     'content', jsonb_build_array(
                                         jsonb_build_object('type', 'text', 'text', line)))
                            ELSE jsonb_build_object('type', 'paragraph')
                       END
                       ORDER BY n)
            -- Blank lines around the text are dropped; blank lines inside it stay, as the
            -- empty paragraphs the vet put there.
            FROM regexp_split_to_table(
                     regexp_replace(
                         regexp_replace(plain, '\r\n?', E'\n', 'g'),
                         '^(\s*\n)+|(\n\s*)+$', '', 'g'),
                     E'\n') WITH ORDINALITY AS lines(line, n)))
    END
$$;

-- The words of a document: one line per paragraph (list items included, without their
-- markers), formatting dropped. What a text block is searched by, and what tells a document
-- with text from one without.
CREATE FUNCTION rich_text_plain(doc JSONB) RETURNS TEXT
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT string_agg(
               (SELECT coalesce(string_agg(
                           CASE WHEN inline ->> 'type' = 'text' THEN inline ->> 'text'
                                ELSE E'\n'
                           END, '' ORDER BY i), '')
                FROM jsonb_array_elements(coalesce(paragraph -> 'content', '[]'))
                     WITH ORDINALITY AS inlines(inline, i)),
               E'\n' ORDER BY p)
    FROM jsonb_path_query(doc, 'strict $.** ? (@.type == "paragraph")')
         WITH ORDINALITY AS paragraphs(paragraph, p)
$$;

-- The index on the text would not survive the column becoming JSONB; it moves to
-- `content_text` below.
DROP INDEX text_block_content_trgm_idx;

ALTER TABLE patient_treatment
    ALTER COLUMN treatment_reason TYPE JSONB USING rich_text_from_plain(treatment_reason),
    ALTER COLUMN finding TYPE JSONB USING rich_text_from_plain(finding),
    ADD CONSTRAINT patient_treatment_reason_document CHECK (
        treatment_reason IS NULL
        OR (treatment_reason ->> 'type' = 'doc' AND rich_text_plain(treatment_reason) ~ '\S')
    ),
    ADD CONSTRAINT patient_treatment_finding_document CHECK (
        finding IS NULL OR (finding ->> 'type' = 'doc' AND rich_text_plain(finding) ~ '\S')
    );

ALTER TABLE text_block
    ALTER COLUMN content TYPE JSONB USING rich_text_from_plain(content),
    ADD CONSTRAINT text_block_content_document CHECK (
        content IS NULL OR (content ->> 'type' = 'doc' AND rich_text_plain(content) ~ '\S')
    ),
    -- Derived here rather than written by the application, so a search can never disagree
    -- with the content it searches.
    ADD COLUMN content_text TEXT GENERATED ALWAYS AS (rich_text_plain(content)) STORED;

CREATE INDEX text_block_content_text_trgm_idx ON text_block USING gin (content_text gin_trgm_ops);
