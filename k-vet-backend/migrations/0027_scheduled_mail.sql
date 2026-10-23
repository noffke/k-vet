-- Invoice mail can wait until a chosen time.
--
-- After a euthanasia the vet finds an invoice arriving straight away unkind to the grieving
-- owners, yet wants the visit documented and the invoice written and released the same day, so
-- it is off her mind. A queued mail now carries the moment it may go out: the queue's run takes
-- it with the first run at or after it. "Send now" is simply now. The six hours of retrying
-- still count from the first failure, which cannot come before this moment.
ALTER TABLE mail_queue ADD COLUMN send_after TIMESTAMPTZ NOT NULL DEFAULT now();
