-- The codes printed on a drug's packs, so a barcode scanner finds the drug (Wareneingang).
--
-- Every code is kept in one form: the 14-digit GTIN. An EAN-13 is a GTIN with a leading zero;
-- a PZN — from its Code 39 barcode, or inside the PPN of an IFA Data Matrix — becomes its NTIN,
-- "04150" + PZN + check digit, the GTIN the securPharm coding rules give every PZN. So the EAN,
-- the PZN barcode and either Data Matrix of one pack meet in the same row.
--
-- Only what identifies the product is kept. Batch, expiry and serial change with every
-- delivery (the serial with every pack): they belong to the lot booked in a Wareneingang.
--
-- A code names one drug, a drug has any number: its packs may carry an EAN and a PZN, and a
-- manufacturer may change the EAN without the drug changing. Codes hang off the drug rather
-- than a packaging: the one original packaging is what is bought and stocked.
CREATE TABLE drug_code (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    drug_id    BIGINT NOT NULL REFERENCES drug (id) ON DELETE CASCADE,
    gtin       TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drug_code_gtin_format CHECK (gtin ~ '^[0-9]{14}$')
);

CREATE INDEX drug_code_drug_idx ON drug_code (drug_id);
