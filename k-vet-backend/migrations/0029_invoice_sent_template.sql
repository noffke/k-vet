-- The email template an invoice went out with.
--
-- The vet picks a template when releasing or sending (migration 0028); the queue keeps that
-- choice only until the mail is delivered. The delivery itself is recorded on the invoice —
-- time and addresses (0023) — and now which text, written together with them once the mail
-- server took the message, so the record says what was delivered, not what was asked for.
-- Invoices emailed before stay empty: there was a single template then, but which name it would
-- have in today's configuration cannot be known after the fact.
ALTER TABLE invoice
    ADD COLUMN sent_email_template TEXT,
    ADD CONSTRAINT invoice_sent_template_with_email
        CHECK (sent_email_template IS NULL OR ts_sent_email IS NOT NULL);
