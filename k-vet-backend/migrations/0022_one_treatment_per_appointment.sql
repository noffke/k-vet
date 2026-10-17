-- One treatment per appointment, and it is always there.
--
-- The vet decided that several treatments under one Termin made things harder, not easier: a
-- second invoice for the same visit now means a second appointment with the same date, time
-- and customer. With that, the treatment stops being something to create — it is the other half
-- of the appointment, made with it, and the two screens become one. The tables stay separate:
-- the appointment carries the draft lifecycle (time, note, customer), the treatment is the
-- record that invoices, stock movements and lots hang off, and that one is born complete.

-- Moving and repairing rows below must not count as editing them: `treatment.updated_at` is
-- what tells an invoice PDF it is out of date, and none of these rows' content changes.
ALTER TABLE treatment DISABLE TRIGGER treatment_set_updated_at;

-- 1. Split. Every treatment but the oldest of an appointment moves to its own copy of that
--    appointment — same time, note and customer — exactly what the vet will do by hand from now
--    on. The copy shares the customer, so the composite key to it holds.
DO $$
DECLARE
    extra RECORD;
    copy_id BIGINT;
BEGIN
    FOR extra IN
        SELECT treatment.id, treatment.appointment_id
          FROM treatment
         WHERE treatment.id > (SELECT min(oldest.id) FROM treatment oldest
                                WHERE oldest.appointment_id = treatment.appointment_id)
         ORDER BY treatment.id
    LOOP
        INSERT INTO appointment (starts_at, note, customer_id, draft, created_at, updated_at)
        SELECT starts_at, note, customer_id, draft, created_at, updated_at
          FROM appointment WHERE id = extra.appointment_id
        RETURNING id INTO copy_id;

        UPDATE treatment SET appointment_id = copy_id WHERE id = extra.id;
    END LOOP;
END
$$;

-- 2. Repair copies made by the old treatment duplication, which left the customer out of the
--    copied treatment and its animals' records: such a treatment could not be billed, and its
--    animal picker offered the whole practice.
UPDATE treatment
   SET customer_id = appointment.customer_id
  FROM appointment
 WHERE appointment.id = treatment.appointment_id
   AND treatment.customer_id IS NULL
   AND appointment.customer_id IS NOT NULL;

UPDATE patient_treatment record
   SET customer_id = patient.customer_id
  FROM patient, treatment
 WHERE patient.id = record.patient_id
   AND treatment.id = record.treatment_id
   AND record.customer_id IS NULL
   AND patient.customer_id = treatment.customer_id;

-- 3. Backfill. An appointment without a treatment gets its (empty) one now.
INSERT INTO treatment (appointment_id, customer_id, created_at, updated_at)
SELECT appointment.id, appointment.customer_id, appointment.created_at, appointment.created_at
  FROM appointment
 WHERE NOT EXISTS (SELECT 1 FROM treatment WHERE treatment.appointment_id = appointment.id);

ALTER TABLE treatment ENABLE TRIGGER treatment_set_updated_at;

-- 4. From here on the database holds it: one treatment per appointment.
DROP INDEX treatment_appointment_idx;
ALTER TABLE treatment ADD CONSTRAINT treatment_appointment_id_key UNIQUE (appointment_id);

-- 5. The customer is chosen on the appointment and follows to the treatment. With ON UPDATE
--    CASCADE a change from one customer to another flows down by itself; once an animal is on
--    the treatment, `patient_treatment_treatment_customer_fkey` (no ON UPDATE) refuses it — the
--    customer can change until an animal is attached, and the database says so on its own.
--    A first customer (NULL before) is not a key change the cascade sees; the application sets
--    the treatment's customer for that case in the same transaction.
ALTER TABLE treatment
    DROP CONSTRAINT treatment_appointment_customer_fkey,
    ADD CONSTRAINT treatment_appointment_customer_fkey
        FOREIGN KEY (appointment_id, customer_id) REFERENCES appointment (id, customer_id)
        ON DELETE CASCADE ON UPDATE CASCADE;
