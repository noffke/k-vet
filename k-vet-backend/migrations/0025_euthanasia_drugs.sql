-- Euthanasia drugs: dispensing one ends the animal's record when the invoice is released.
--
-- An animal put down at a visit stayed alive in k-vet until the vet remembered to enter the
-- date of death — and until then it was offered for the next appointment, counted among the
-- customer's animals and kept on the reminders. A drug marked here (application code reads it
-- when an invoice is released) sets the date of death of the animal it was dispensed for to the
-- day of the appointment.
ALTER TABLE drug ADD COLUMN euthanasia BOOLEAN NOT NULL DEFAULT false;

-- The practice's euthanasia preparation is "Release" (pentobarbital), under whatever strength
-- or pack it was entered.
UPDATE drug SET euthanasia = true WHERE name ILIKE '%release%';
