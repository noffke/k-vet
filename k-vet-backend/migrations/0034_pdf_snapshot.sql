-- What an invoice PDF showed, kept with the invoice, so the app can say not only *that* the
-- treatment changed since the PDF was made but *what* (issues.md): which lines are new or
-- different, how many are gone, whose texts changed, whether the order moved.
--
-- A timestamp cannot tell that: renumbering after a deletion touches every line below it, a
-- line changed and changed back is no change, and a finding the invoice does not print is
-- no reason to warn. So each printed line and each animal's printed texts get a fingerprint,
-- and the PDF's snapshot is compared with the treatment's as it is now.

-- The treatment as an invoice prints it. `with_finding` follows the invoice's "Befund
-- aufführen": a finding it does not print is not part of what it shows.
CREATE FUNCTION print_snapshot(treatment_id BIGINT, with_finding BOOLEAN) RETURNS JSONB
LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object(
        'items', COALESCE((
            SELECT jsonb_object_agg(item.id::text, md5(jsonb_build_array(
                       item.patient_treatment_id, item.kind, item.name, item.quantity,
                       item.unit, item.factor, item.got_number, item.price_net,
                       item.vat_percent, item.km, item.km_multiplier, item.redesignation
                   )::text))
              FROM treatment_item item
             WHERE item.treatment_id = $1), '{}'::jsonb),
        'records', COALESCE((
            SELECT jsonb_object_agg(record.id::text, md5(jsonb_build_array(
                       record.patient_id, record.treatment_reason,
                       CASE WHEN $2 THEN record.finding END
                   )::text))
              FROM patient_treatment record
             WHERE record.treatment_id = $1), '{}'::jsonb),
        'order', COALESCE((
            SELECT jsonb_agg(item.id ORDER BY item.patient_treatment_id NULLS LAST,
                                              item.position, item.id)
              FROM treatment_item item
             WHERE item.treatment_id = $1), '[]'::jsonb)
    )
$$;

-- Taken when the invoice's document is built, in the same transaction as what it is built from.
ALTER TABLE invoice ADD COLUMN pdf_snapshot JSONB;

-- Whether the invoice's PDF no longer shows its treatment as it is. NULL without a PDF.
-- An invoice from before the snapshot falls back to the timestamps it was judged by until now.
CREATE FUNCTION invoice_pdf_out_of_date(invoice_id BIGINT) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT CASE
               WHEN invoice.pdf_snapshot IS NOT NULL THEN
                   invoice.pdf_snapshot
                       <> print_snapshot(invoice.treatment_id, invoice.includes_finding)
               ELSE treatment.updated_at > attachment.created_at
           END
      FROM invoice
      JOIN treatment ON treatment.id = invoice.treatment_id
      JOIN attachment ON attachment.id = invoice.pdf_attachment_id
     WHERE invoice.id = $1
$$;

-- An existing PDF that still matches by the old rule shows the treatment as it is now: that is
-- its snapshot. One already out of date keeps the old rule, and says so without detail.
UPDATE invoice
   SET pdf_snapshot = print_snapshot(invoice.treatment_id, invoice.includes_finding)
  FROM treatment, attachment
 WHERE treatment.id = invoice.treatment_id
   AND attachment.id = invoice.pdf_attachment_id
   AND treatment.updated_at <= attachment.created_at;
