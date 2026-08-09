-- The dispatch timestamps, now that 0015's `sent` value exists and can be written into a CHECK.
--
-- `ts_sent_email` (0006) is written only after the mail server accepted the message;
-- `ts_sent_post` records a hand-over on paper, which nothing else can observe. The status says
-- where the invoice is, the timestamps say when it went and by which route (FR-035), and the
-- constraints keep the two from drifting apart.

ALTER TABLE invoice
    ADD COLUMN ts_sent_post TIMESTAMPTZ,
    ADD CONSTRAINT invoice_sent_timestamp CHECK (
        status <> 'sent' OR ts_sent_email IS NOT NULL OR ts_sent_post IS NOT NULL
    );

-- Bookkeeping only ever receives invoices the customer already has: submission implies dispatch.
ALTER TABLE invoice
    DROP CONSTRAINT invoice_submitted_timestamp,
    ADD CONSTRAINT invoice_submitted_timestamp CHECK (
        status <> 'submitted' OR (
            ts_accepted IS NOT NULL AND ts_submitted IS NOT NULL
            AND (ts_sent_email IS NOT NULL OR ts_sent_post IS NOT NULL)
        )
    );
