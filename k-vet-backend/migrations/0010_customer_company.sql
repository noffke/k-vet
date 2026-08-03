-- Optional company name on a customer's addresses.
--
-- Some customers are businesses — a breeder, a shelter, a farm — and the invoice has to name the
-- company as well as the person. Until now the only place to put it was `home_addon`, which
-- prints *below* the name and is meant for `c/o`-style additions.
--
-- One field per address block, mirroring `first_name`/`invoice_first_name`: `pdf::address_block`
-- branches completely between the two recipients, so a single shared column would either be
-- dropped on the invoice-address path or wrongly attached to a different recipient.
--
-- Nullable and deliberately absent from the `customer_complete` CHECK and from the
-- `has_invoice_address` generated column: the company is optional, and requiring it would make
-- every private customer incomplete. First and last name stay mandatory, so there is never a
-- company-only recipient and the salutation logic is untouched.

ALTER TABLE customer
    ADD COLUMN company         TEXT,
    ADD COLUMN invoice_company TEXT;
