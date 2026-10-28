-- The customers as an address book on the vet's phone (CardDAV at /dav, served by k-vet). Her
-- phone book shows who is calling and finds a customer by name; kept in her Google account,
-- Google would hold them. Contacts can be made and edited on the phone too.

-- A second number. Without it, a mobile number added on the phone next to the landline would
-- vanish again at the next sync, k-vet having nowhere to keep it.
ALTER TABLE customer ADD COLUMN mobile TEXT;

-- The phone's own names for a contact it created, as for the calendar's entries (0036).
ALTER TABLE customer
    ADD COLUMN dav_uid    TEXT UNIQUE,
    ADD COLUMN dav_href   TEXT UNIQUE,
    ADD COLUMN dav_origin TEXT NOT NULL DEFAULT 'kvet',
    ADD CONSTRAINT customer_dav_origin CHECK (dav_origin IN ('kvet', 'phone')),
    ADD CONSTRAINT customer_dav_phone_names
        CHECK (dav_origin = 'kvet' OR (dav_uid IS NOT NULL AND dav_href IS NOT NULL));
