-- "Kein Medikament" (2026-11-21): a product the practice sells that is not an Arzneimittel —
-- food, shampoo, a supplement. The AMPreisV does not price it: the vet types the net sales price,
-- and the gross follows from the VAT rate. A Teilmenge is proposed pro rata from the pack's net
-- price, without the § 4 mark-up, and may be typed over (`drugs::recompute_prices`).
--
-- Until now "Ohne Bestand" (migration 0043) carried this meaning as well. The two are separate
-- from here on: "Ohne Bestand" only says that no stock is kept. Every drug that was "Ohne
-- Bestand" is a consumable that is not a medicine, so it becomes "Kein Medikament" too and keeps
-- the price the vet typed.
ALTER TABLE drug ADD COLUMN not_a_medicine BOOLEAN NOT NULL DEFAULT false;

UPDATE drug SET not_a_medicine = true WHERE without_stock;

-- What dispensing needs to know: a "beliebig teilbar" product without a Teilmenge bills its rest
-- pro rata of the pack price instead of as an AMPreisV Teilmenge (`dispense::pricing`).
CREATE OR REPLACE VIEW drug_dispense_rule AS
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
       smallest.sales_price_net  AS subset_price_net,
       drug.not_a_medicine       AS not_a_medicine
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
