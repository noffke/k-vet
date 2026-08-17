-- A self-defined service may name the GOT position it is charged analogously to (§ 8 GOT).
--
-- The vet is allowed to define her own Leistungen on the basis of an existing GOT position, and
-- the invoice should say which one — the chain already carries `got_number` from the service
-- through the treatment line to the "GOT-Nr" line on the PDF. Until now the shape constraint
-- forbade it: a `self_defined` row had to leave the number NULL.
--
-- What stays: a `got` position is still incomplete without its number and factor. What changes:
-- for a self-defined one the number is simply optional, and its presence is what marks the
-- position as an analogous charge — no second column that could disagree with it.

ALTER TABLE service
    DROP CONSTRAINT service_got_shape,
    ADD CONSTRAINT service_got_shape CHECK (
        draft OR type <> 'got' OR (got_number IS NOT NULL AND factor IS NOT NULL)
    );
