-- Antibiotics: marked on the drug, for now only so the vet can see it — like the vaccine and
-- refrigeration marks, nothing reads it yet.
ALTER TABLE drug ADD COLUMN antibiotic BOOLEAN NOT NULL DEFAULT false;
