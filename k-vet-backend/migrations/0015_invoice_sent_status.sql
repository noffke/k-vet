-- `sent` joins the invoice lifecycle between acceptance and the bookkeeping hand-off.
--
-- An invoice that never reached the customer is money the practice loses, and until now it was
-- indistinguishable from one that did: `accept` sends the email as a side effect and swallows a
-- failing SMTP connection. Dispatch becomes a state of its own, and a precondition of handing
-- the invoice to the bookkeeper.
--
-- Postgres refuses to *use* an enum value in the transaction that added it, and sqlx runs each
-- migration file in its own transaction — so this file adds the value and nothing else. The
-- column and the constraint that reference it live in 0016.

ALTER TYPE invoice_status ADD VALUE 'sent' AFTER 'accepted';
