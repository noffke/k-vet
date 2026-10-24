-- The email template a queued mail goes out with.
--
-- The vet now picks one of the templates listed in the configuration when releasing or sending
-- an invoice. The queue keeps the template's name — the configuration's identifier for it — and
-- renders with it when the mail goes out. A mail queued before there was a choice has none,
-- which means the first template listed: the one that was always used.
ALTER TABLE mail_queue ADD COLUMN template TEXT;
