-- The name on the practice's bank account: the "Kontoinhaber" an invoice's bank details and its
-- GiroCode name as the recipient of the transfer. It is not necessarily the practice's name — a
-- practice trading under one name may bank under the vet's own. Empty falls back to the practice
-- name (`invoices::build_document`), which is what every invoice used until now.
ALTER TABLE global_settings ADD COLUMN account_holder TEXT NOT NULL DEFAULT '';
