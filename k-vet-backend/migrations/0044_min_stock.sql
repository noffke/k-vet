-- A Mindestbestand per drug (issues.md, 2026-11-18): below it, the dashboard lists the drug to
-- reorder. Optional, and counted in whole original packagings — a positive multiple of the
-- Originalpackung by construction, which follows a changed pack size rather than turning into
-- an odd number of millilitres.
ALTER TABLE drug ADD COLUMN min_stock_packs INTEGER;

ALTER TABLE drug ADD CONSTRAINT drug_min_stock_positive
    CHECK (min_stock_packs IS NULL OR min_stock_packs > 0);

-- A consumable "Ohne Bestand" (migration 0043) keeps no stock to fall short of.
ALTER TABLE drug ADD CONSTRAINT drug_min_stock_needs_stock
    CHECK (NOT (without_stock AND min_stock_packs IS NOT NULL));
