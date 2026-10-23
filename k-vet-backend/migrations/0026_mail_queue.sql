-- Invoice mail goes through a queue.
--
-- A mail used to be sent inside the request that released the invoice or asked for it: a
-- failure there was logged and forgotten (on release), or reported as "the mail server is not
-- reachable" whatever had actually gone wrong (on "E-Mail senden"), and a mail server that
-- swallowed the connection held the button for a minute. Now a request only queues the mail;
-- an in-process job tries what is waiting every five minutes, keeps the error on the row,
-- retries for six hours after the first failure and stops at once when the mail server refuses
-- for good. The dashboard lists what failed, to retry or remove.
--
-- The table holds only what has not gone out: a delivered mail's row is deleted, and the
-- delivery stays recorded where it always was (`invoice.ts_sent_email`, `sent_email_*`, status
-- `sent`). Template, CC/BCC and PDF are read when the mail is sent, as before, so a row is just
-- the invoice and the addresses asked for.
CREATE TABLE mail_queue (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- One waiting mail per invoice: asking again replaces it rather than queuing a second.
    invoice_id       BIGINT NOT NULL UNIQUE REFERENCES invoice (id) ON DELETE CASCADE,
    recipients       TEXT[] NOT NULL,
    queued_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts         INTEGER NOT NULL DEFAULT 0,
    last_attempt_at  TIMESTAMPTZ,
    first_failure_at TIMESTAMPTZ,
    -- The transport's own words — they name the host or carry the mail server's reply.
    last_error       TEXT,
    -- The last failure was final (a 5xx reply, or a mail k-vet cannot build): not retried.
    refused          BOOLEAN NOT NULL DEFAULT false,
    gave_up_at       TIMESTAMPTZ,
    CONSTRAINT mail_queue_has_recipients CHECK (cardinality(recipients) > 0),
    CONSTRAINT mail_queue_failure_shape CHECK ((last_error IS NULL) = (first_failure_at IS NULL)),
    CONSTRAINT mail_queue_gave_up_after_failure CHECK (gave_up_at IS NULL OR last_error IS NOT NULL),
    CONSTRAINT mail_queue_attempts_not_negative CHECK (attempts >= 0)
);
