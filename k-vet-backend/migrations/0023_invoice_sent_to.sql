-- Where an emailed invoice actually went.
--
-- `email_recipients` holds the addresses last *asked for*: it is written before the mail server
-- is tried, so that a retry uses what the vet just typed. That makes it the wrong thing to show
-- as "sent to" — after a failed resend to new addresses it names people who never got the mail.
-- And CC/BCC are read from the settings at send time and stored nowhere, so a later change to
-- the settings rewrote history. These three are written only together with `ts_sent_email`,
-- after the mail server has accepted the message.
ALTER TABLE invoice
    ADD COLUMN sent_email_to  TEXT[],
    ADD COLUMN sent_email_cc  TEXT[],
    ADD COLUMN sent_email_bcc TEXT[];

-- Invoices already sent: the requested addresses are the best knowledge there is — they were
-- the ones used by the last successful send unless a later attempt failed. CC/BCC stay unknown.
UPDATE invoice
   SET sent_email_to = email_recipients
 WHERE ts_sent_email IS NOT NULL AND email_recipients IS NOT NULL;
