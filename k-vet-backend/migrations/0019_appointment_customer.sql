-- The customer is chosen, not guessed (issues.md 7).
--
-- Until now nothing recorded whose visit an appointment was. The invoice's customer was read
-- off `patients.first()` after ordering by name, and the rule that every animal on a treatment
-- shares one customer (FR-027) lived only in `attach_patient` — where it engaged *after* the
-- first animal was attached, so an empty treatment offered every animal in the practice, and a
-- treatment that did end up mixed would silently have invoiced whichever owner sorted first.
--
-- The customer now hangs on the appointment, is carried down to the treatment, and is enforced
-- by composite foreign keys rather than by the handler alone (Constitution IV). The columns are
-- nullable because appointments are born as empty drafts; a foreign key with a NULL part is not
-- checked, which is exactly the freedom a draft needs.

-- Refuse to migrate a practice whose data already breaks the rule, rather than letting the
-- foreign keys below fail with something unreadable. The handler should have prevented this,
-- but this is the moment to find out.
DO $$
DECLARE
    mixed TEXT;
BEGIN
    SELECT string_agg(treatment_id::TEXT, ', ')
      INTO mixed
      FROM (
          SELECT record.treatment_id
            FROM patient_treatment record
            JOIN patient ON patient.id = record.patient_id
           WHERE patient.customer_id IS NOT NULL
           GROUP BY record.treatment_id
          HAVING count(DISTINCT patient.customer_id) > 1
      ) offenders;

    IF mixed IS NOT NULL THEN
        RAISE EXCEPTION
            'treatments % have animals from more than one customer; split them before migrating',
            mixed;
    END IF;
END
$$;

ALTER TABLE appointment ADD COLUMN customer_id BIGINT REFERENCES customer (id);
ALTER TABLE treatment ADD COLUMN customer_id BIGINT REFERENCES customer (id);
ALTER TABLE patient_treatment ADD COLUMN customer_id BIGINT REFERENCES customer (id);

-- Backfill from what the code was deriving anyway: the animals already on the record.
UPDATE patient_treatment record
   SET customer_id = patient.customer_id
  FROM patient
 WHERE patient.id = record.patient_id;

UPDATE treatment
   SET customer_id = (
       SELECT record.customer_id
         FROM patient_treatment record
        WHERE record.treatment_id = treatment.id
          AND record.customer_id IS NOT NULL
        LIMIT 1
   );

-- A Termin could hold treatments for more than one customer, and that was never wrong: the
-- rule was only ever that one *treatment* has one customer (FR-027), and nothing tied a Termin
-- to anybody. These are real visits, so refusing them would strand the practice on the old
-- schema for records it was right to have. A Termin carries one customer from here on, so the
-- ones that carry several are split instead: the first customer keeps the Termin, every other
-- one gets a copy of it, and their treatments move across. Time, note and draft state are
-- copied, so the day's list still shows every visit — as two entries where it showed one.
DO $$
DECLARE
    shared  RECORD;
    moved   BIGINT;
    copy_id BIGINT;
BEGIN
    FOR shared IN
        SELECT appointment_id, min(customer_id) AS keeps
          FROM treatment
         WHERE customer_id IS NOT NULL
         GROUP BY appointment_id
        HAVING count(DISTINCT customer_id) > 1
    LOOP
        FOR moved IN
            SELECT DISTINCT customer_id
              FROM treatment
             WHERE appointment_id = shared.appointment_id
               AND customer_id IS NOT NULL
               AND customer_id <> shared.keeps
             ORDER BY 1
        LOOP
            INSERT INTO appointment (starts_at, note, draft, created_at, updated_at)
            SELECT starts_at, note, draft, created_at, updated_at
              FROM appointment
             WHERE id = shared.appointment_id
            RETURNING id INTO copy_id;

            UPDATE treatment
               SET appointment_id = copy_id
             WHERE appointment_id = shared.appointment_id
               AND customer_id = moved;

            -- Named in the start-up log, because this is the one thing the migration changes
            -- that the vet would otherwise discover by finding a second Termin in her day.
            RAISE NOTICE 'Termin % also held customer %; moved those treatments to Termin %',
                shared.appointment_id, moved, copy_id;
        END LOOP;
    END LOOP;
END
$$;

UPDATE appointment
   SET customer_id = (
       SELECT treatment.customer_id
         FROM treatment
        WHERE treatment.appointment_id = appointment.id
          AND treatment.customer_id IS NOT NULL
        LIMIT 1
   );

-- The targets of the composite keys. `patient.customer_id` is nullable, so these are plain
-- unique constraints over a pair rather than anything the rows must fill in.
ALTER TABLE customer ADD CONSTRAINT customer_id_key UNIQUE (id);
ALTER TABLE patient ADD CONSTRAINT patient_id_customer_key UNIQUE (id, customer_id);
ALTER TABLE appointment ADD CONSTRAINT appointment_id_customer_key UNIQUE (id, customer_id);
ALTER TABLE treatment ADD CONSTRAINT treatment_id_customer_key UNIQUE (id, customer_id);

-- A treatment belongs to its appointment's customer.
ALTER TABLE treatment
    ADD CONSTRAINT treatment_appointment_customer_fkey
    FOREIGN KEY (appointment_id, customer_id) REFERENCES appointment (id, customer_id)
    ON DELETE CASCADE;

-- An animal's record belongs to its treatment's customer …
ALTER TABLE patient_treatment
    ADD CONSTRAINT patient_treatment_treatment_customer_fkey
    FOREIGN KEY (treatment_id, customer_id) REFERENCES treatment (id, customer_id)
    ON DELETE CASCADE;

-- … and the animal itself belongs to that same customer. Together these two make a
-- mixed-customer treatment impossible to write, from any code path, including plain SQL.
ALTER TABLE patient_treatment
    ADD CONSTRAINT patient_treatment_patient_customer_fkey
    FOREIGN KEY (patient_id, customer_id) REFERENCES patient (id, customer_id);

CREATE INDEX appointment_customer_idx ON appointment (customer_id);
CREATE INDEX treatment_customer_idx ON treatment (customer_id);
