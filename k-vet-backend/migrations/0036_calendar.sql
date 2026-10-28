-- The appointments as a calendar on the vet's phone (CalDAV at /dav, served by k-vet itself).
-- The phone reads and writes the appointments directly; there is no second copy to keep in
-- step, so all this needs is how the phone names what it made, and what it hid.
ALTER TABLE appointment
    -- The phone's own names for an appointment it created: the iCalendar UID and the file
    -- name it put it under. Absent for an appointment made in k-vet (`kvet-{id}`).
    ADD COLUMN calendar_uid    TEXT UNIQUE,
    ADD COLUMN calendar_href   TEXT UNIQUE,
    ADD COLUMN calendar_origin TEXT NOT NULL DEFAULT 'kvet',
    -- The title it was given on the phone, which names the customer to suggest.
    ADD COLUMN calendar_title  TEXT,
    -- Deleted on the phone but kept here, because something was recorded on it.
    ADD COLUMN calendar_hidden BOOLEAN NOT NULL DEFAULT false,
    ADD CONSTRAINT appointment_calendar_origin CHECK (calendar_origin IN ('kvet', 'phone')),
    ADD CONSTRAINT appointment_calendar_phone_names
        CHECK (calendar_origin = 'kvet' OR (calendar_uid IS NOT NULL AND calendar_href IS NOT NULL));

-- What the phone keeps in the calendar that cannot be an appointment — an all-day entry, a
-- recurring one — kept as it was sent and served back unchanged, so the calendar behaves as a
-- calendar for those too.
CREATE TABLE calendar_extra_event (
    href       TEXT PRIMARY KEY,
    uid        TEXT NOT NULL UNIQUE,
    ics        TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
