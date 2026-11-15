-- A drug position says what was dispensed, not which packaging how often.
--
-- The vet picks a drug — only its original packaging is offered — and enters the quantity in the
-- original's base unit: 12 Tbl. from a pack of 10, 150 ml from a 100 ml bottle, 2,7 ml of an
-- injection. Whole packs are billed at the pack price, the rest as a Teilmenge at the price of
-- the drug's smallest Teilmenge, pro rata (`money::dispense_rows`, requirements/prices.md); the
-- invoice prints the two as two rows.
--
-- Until now a line counted packagings: 3 × "Amoxicillin · 10 ml". Lines already billed keep that
-- form — `pack_size IS NULL` marks it — and everything still open is converted below.

-- "Beliebig teilbar": any rest may be dispensed, not only multiples of the smallest Teilmenge —
-- for injection liquids. Without a Teilmenge of its own such a drug prices the rest as its share
-- of the whole pack priced as a Teilmenge (`money::whole_pack_as_subset`).
ALTER TABLE drug ADD COLUMN freely_divisible BOOLEAN NOT NULL DEFAULT false;

-- How each drug can be dispensed today: its original pack and its smallest usable Teilmenge.
-- One definition of "the smallest Teilmenge", for the picker and every place a quantity is typed.
CREATE VIEW drug_dispense_rule AS
SELECT drug.id                   AS drug_id,
       original.id               AS original_id,
       original.unit             AS unit,
       original.quantity         AS pack_size,
       original.sales_price_net  AS pack_price_net,
       original.list_price_net   AS list_price_net,
       original.draft            AS original_draft,
       drug.name                 AS drug_name,
       drug.vat_percent          AS vat_percent,
       drug.human_drug           AS human_drug,
       drug.freely_divisible     AS freely_divisible,
       smallest.quantity         AS subset_size,
       smallest.sales_price_net  AS subset_price_net
  FROM drug
  JOIN drug_packaging original ON original.drug_id = drug.id AND original.kind = 'original'
  LEFT JOIN LATERAL (
      SELECT subset.quantity, subset.sales_price_net
        FROM drug_packaging subset
       WHERE subset.drug_id = drug.id
         AND subset.kind = 'subset'
         AND NOT subset.draft AND NOT subset.archived
         AND subset.quantity > 0
         AND subset.sales_price_net IS NOT NULL
       ORDER BY subset.quantity, subset.id
       LIMIT 1
  ) smallest ON true;

-- What a line in the new form pins besides its pack price (`price_net`): the pack size, the
-- Teilmenge price its rest is billed at (`subset_size` base units cost `subset_price_net`; none:
-- whole packs only) and the drug's flag. Pinned like every billing value, so a later change to
-- the drug moves neither the price nor the steps of a line already entered. Which rest is allowed
-- is checked where a quantity is typed, not here: it is a rule about input.
ALTER TABLE treatment_item
    ADD COLUMN pack_size        NUMERIC(10, 2),
    ADD COLUMN subset_size      NUMERIC(10, 2),
    ADD COLUMN subset_price_net NUMERIC(10, 2),
    ADD COLUMN freely_divisible BOOLEAN NOT NULL DEFAULT false,
    ADD CONSTRAINT treatment_item_dispense_is_a_drug
        CHECK (pack_size IS NULL OR kind = 'drug_packaging'),
    ADD CONSTRAINT treatment_item_pack_size_positive CHECK (pack_size > 0),
    ADD CONSTRAINT treatment_item_subset_pair
        CHECK ((subset_size IS NULL) = (subset_price_net IS NULL)),
    ADD CONSTRAINT treatment_item_subset_needs_pack
        CHECK (subset_size IS NULL OR pack_size IS NOT NULL),
    ADD CONSTRAINT treatment_item_subset_size_positive CHECK (subset_size > 0),
    ADD CONSTRAINT treatment_item_subset_price_not_negative CHECK (subset_price_net >= 0),
    ADD CONSTRAINT treatment_item_divisible_needs_subset
        CHECK (NOT freely_divisible OR subset_size IS NOT NULL);

-- A quote line pins the same, as quoted — and the unit, which it never had.
ALTER TABLE price_quote_line
    ADD COLUMN unit             TEXT,
    ADD COLUMN pack_size        NUMERIC(10, 2),
    ADD COLUMN subset_size      NUMERIC(10, 2),
    ADD COLUMN subset_price_net NUMERIC(12, 2),
    ADD COLUMN freely_divisible BOOLEAN NOT NULL DEFAULT false,
    ADD CONSTRAINT price_quote_line_dispense_is_a_drug
        CHECK (pack_size IS NULL OR kind = 'drug_packaging'),
    ADD CONSTRAINT price_quote_line_pack_size_positive CHECK (pack_size > 0),
    ADD CONSTRAINT price_quote_line_subset_pair
        CHECK ((subset_size IS NULL) = (subset_price_net IS NULL)),
    ADD CONSTRAINT price_quote_line_subset_needs_pack
        CHECK (subset_size IS NULL OR pack_size IS NOT NULL),
    ADD CONSTRAINT price_quote_line_subset_size_positive CHECK (subset_size > 0),
    ADD CONSTRAINT price_quote_line_subset_price_not_negative CHECK (subset_price_net >= 0),
    ADD CONSTRAINT price_quote_line_divisible_needs_subset
        CHECK (NOT freely_divisible OR subset_size IS NOT NULL);

-- ── Converting what is still open ────────────────────────────────────────────────────────────
--
-- Only stored values are copied; no price is computed here (money is computed in `money.rs`).
-- A line of a Teilmenge keeps its pinned price as the Teilmenge price of its rest and takes the
-- original's current price for whole packs; a line of the original keeps its pinned pack price
-- and takes the drug's current smallest Teilmenge for a rest. No drug is "beliebig teilbar" yet,
-- so no Teilmenge price ever has to be derived. The quantity becomes base units — exactly what
-- the line's draft dispenses already hold, so the stock ledger is untouched.

-- Like 0024: what was dispensed does not change, so the lines' and treatments' timestamps do not
-- either. An invoice PDF that no longer matches says so through its print snapshot.
ALTER TABLE treatment_item DISABLE TRIGGER treatment_item_set_updated_at;
ALTER TABLE treatment_item DISABLE TRIGGER treatment_item_touches_treatment;

-- Open: no invoice of the treatment is accepted, sent or submitted (`stock::treatment_is_frozen`).
UPDATE treatment_item item
   SET quantity         = item.quantity * billed.quantity,
       drug_packaging_id = rule.original_id,
       unit             = COALESCE(rule.unit, item.unit),
       name             = COALESCE(rule.drug_name, item.name),
       pack_size        = rule.pack_size,
       price_net        = CASE WHEN billed.kind = 'original' THEN item.price_net
                               ELSE rule.pack_price_net END,
       subset_size      = CASE WHEN billed.kind = 'subset' THEN billed.quantity
                               ELSE rule.subset_size END,
       subset_price_net = CASE WHEN billed.kind = 'subset' THEN item.price_net
                               ELSE rule.subset_price_net END
  FROM drug_packaging billed
  JOIN drug_dispense_rule rule ON rule.drug_id = billed.drug_id
 WHERE billed.id = item.drug_packaging_id
   AND item.kind = 'drug_packaging'
   AND billed.quantity > 0
   AND rule.pack_size > 0
   AND (billed.kind = 'original' OR rule.pack_price_net IS NOT NULL)
   AND NOT EXISTS (
       SELECT 1 FROM invoice
        WHERE invoice.treatment_id = item.treatment_id
          AND invoice.status IN ('accepted', 'sent', 'submitted')
   );

ALTER TABLE treatment_item ENABLE TRIGGER treatment_item_set_updated_at;
ALTER TABLE treatment_item ENABLE TRIGGER treatment_item_touches_treatment;

-- Quotes are never billed: every line converts, at the price it was quoted.
UPDATE price_quote_line line
   SET quantity         = line.quantity * billed.quantity,
       drug_packaging_id = rule.original_id,
       unit             = rule.unit,
       name             = COALESCE(rule.drug_name, line.name),
       pack_size        = rule.pack_size,
       price_net        = CASE WHEN billed.kind = 'original' THEN line.price_net
                               ELSE rule.pack_price_net END,
       subset_size      = CASE WHEN billed.kind = 'subset' THEN billed.quantity
                               ELSE rule.subset_size END,
       subset_price_net = CASE WHEN billed.kind = 'subset' THEN line.price_net
                               ELSE rule.subset_price_net END
  FROM drug_packaging billed
  JOIN drug_dispense_rule rule ON rule.drug_id = billed.drug_id
 WHERE billed.id = line.drug_packaging_id
   AND line.kind = 'drug_packaging'
   AND billed.quantity > 0
   AND rule.pack_size > 0
   AND (billed.kind = 'original' OR rule.pack_price_net IS NOT NULL);

-- Behandlungsgruppen and Mitbringen pin no prices: only the quantity and the packaging change.
UPDATE treatment_template_item item
   SET quantity          = item.quantity * billed.quantity,
       drug_packaging_id = rule.original_id,
       unit              = COALESCE(rule.unit, item.unit)
  FROM drug_packaging billed
  JOIN drug_dispense_rule rule ON rule.drug_id = billed.drug_id
 WHERE billed.id = item.drug_packaging_id
   AND item.kind = 'drug_packaging'
   AND billed.quantity > 0;

UPDATE appointment_reminder reminder
   SET quantity          = reminder.quantity * billed.quantity,
       drug_packaging_id = rule.original_id
  FROM drug_packaging billed
  JOIN drug_dispense_rule rule ON rule.drug_id = billed.drug_id
 WHERE billed.id = reminder.drug_packaging_id
   AND billed.quantity > 0;

COMMENT ON COLUMN appointment_reminder.quantity IS
    'Base units of the original packaging (Tbl., ml), as a drug line counts them since 0039.';

-- ── What an invoice PDF showed ───────────────────────────────────────────────────────────────
--
-- The new pins are part of what a line prints. A line in the old form is fingerprinted exactly as
-- before, so an invoice PDF made before this migration is not reported as out of date for it.
CREATE OR REPLACE FUNCTION print_snapshot(treatment_id BIGINT, with_finding BOOLEAN) RETURNS JSONB
LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object(
        'items', COALESCE((
            SELECT jsonb_object_agg(item.id::text, md5((
                       CASE WHEN item.pack_size IS NULL THEN jsonb_build_array(
                           item.patient_treatment_id, item.kind, item.name, item.quantity,
                           item.unit, item.factor, item.got_number, item.price_net,
                           item.vat_percent, item.km, item.km_multiplier, item.redesignation
                       ) ELSE jsonb_build_array(
                           item.patient_treatment_id, item.kind, item.name, item.quantity,
                           item.unit, item.factor, item.got_number, item.price_net,
                           item.vat_percent, item.km, item.km_multiplier, item.redesignation,
                           item.pack_size, item.subset_size, item.subset_price_net,
                           item.freely_divisible
                       ) END
                   )::text))
              FROM treatment_item item
             WHERE item.treatment_id = $1), '{}'::jsonb),
        'records', COALESCE((
            SELECT jsonb_object_agg(record.id::text, md5(jsonb_build_array(
                       record.patient_id, record.treatment_reason,
                       CASE WHEN $2 THEN record.finding END
                   )::text))
              FROM patient_treatment record
             WHERE record.treatment_id = $1), '{}'::jsonb),
        'order', COALESCE((
            SELECT jsonb_agg(item.id ORDER BY item.patient_treatment_id NULLS LAST,
                                              item.position, item.id)
              FROM treatment_item item
             WHERE item.treatment_id = $1), '[]'::jsonb)
    )
$$;
