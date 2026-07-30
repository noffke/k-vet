-- Net-first prices, structured practice address, contact/bank details and the due date.
--
-- Why the renames: the GOT publishes **net** fees and 0008 imported them faithfully — into a
-- column called `gross_price`. Everything downstream then treated those values as gross and
-- *extracted* VAT from them instead of adding it, so every GOT position was billed roughly 16 %
-- too low (GOT 16: 23,62 € charged where 28,11 € is correct — cross-checked against the
-- practice's own RE-289). The stored values were right all along; the name and the interpretation
-- were wrong.
--
-- Making net the authoritative unit also matches how the domain actually computes: AMPreisV § 3(2)
-- levies its surcharges on the net purchase price, § 14 UStG states an invoice as Entgelt plus
-- Steuerbetrag, and bookkeeping and EN 16931 both work net-first. The gross is derived for display.

ALTER TABLE service        RENAME COLUMN gross_price       TO net_price;
ALTER TABLE drug_packaging RENAME COLUMN sales_price_gross TO sales_price_net;
ALTER TABLE treatment_item RENAME COLUMN price_gross       TO price_net;

-- The sales price of a packaging is the AMPreisV net price, which `money::price_from` already
-- computes and used to discard. Existing rows held that value plus VAT, so take it back out once.
UPDATE drug_packaging
   SET sales_price_net = round(sales_price_net / (1 + COALESCE(
           (SELECT drug.vat_percent FROM drug WHERE drug.id = drug_packaging.drug_id), 0
       ) / 100), 2)
 WHERE sales_price_net IS NOT NULL;

-- `service.net_price` needs no correction: 0008 already stores the published GOT net fees.

-- ── Practice contact and bank details ───────────────────────────────────────────────────────
-- Printed in the invoice footer; the BIC additionally feeds the GiroCode.
ALTER TABLE global_settings
    ADD COLUMN bic       TEXT NOT NULL DEFAULT '',
    ADD COLUMN bank_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN email     TEXT NOT NULL DEFAULT '';

-- ── Structured addresses ────────────────────────────────────────────────────────────────────
-- `practice_address` was one free-text blob. The invoice needs a one-line form for the footer,
-- and an e-invoice (EN 16931 BT-35/37/38/40) needs the parts separately.
ALTER TABLE global_settings
    DROP COLUMN practice_address,
    ADD COLUMN practice_street  TEXT NOT NULL DEFAULT '',
    ADD COLUMN practice_zip     TEXT NOT NULL DEFAULT '',
    ADD COLUMN practice_city    TEXT NOT NULL DEFAULT '',
    ADD COLUMN practice_country TEXT;

ALTER TABLE customer
    ADD COLUMN home_country    TEXT,
    ADD COLUMN invoice_country TEXT;

-- The country columns carry no DEFAULT and no CHECK on purpose. "DE" is a business preference,
-- not an invariant, so it lives in `[invoice] default_country` and NULL means "use the configured
-- default", resolved in Rust. Validation is a real ISO 3166-1 lookup in the API — a CHECK could
-- only match the shape of a code, not whether the country exists. Country stays out of
-- `customer_complete` and `has_invoice_address` for the same reason.

-- ── Due date ────────────────────────────────────────────────────────────────────────────────
-- Pinned at creation from `[invoice] payment_terms_days`, like every other billing-relevant
-- value: changing the configured term later must not move the due date of an issued invoice.
ALTER TABLE invoice ADD COLUMN due_date DATE;
