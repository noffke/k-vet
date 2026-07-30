# Global Settings
Decision criterion: a setting lives in the DB if the vet should be able to change it from the UI without a developer or redeploy; it lives in the config file if it is infrastructure, a secret, structural, or changing it involves a developer anyway.

## In the DB (settings page)
Single-row typed settings table (no key-value store), see Global Settings entity in the datamodel.
* name and address of vet (address structured: street, zip, city, country — an e-invoice needs
  the parts separately, and the invoice footer needs a one-line form)
* email address of the practice (printed in the invoice footer; the sender address of outgoing
  mail stays in the config file, it is infrastructure)
* bank account of vet: IBAN, BIC and bank name (the BIC also feeds the GiroCode on the invoice)
* german UStID
* uploadable logo for vet practice (stored as an Attachment, referenced)
* global CC/BCC for emails (business preference, e.g. bookkeeper's address)

## In the config file
* pattern for invoice number (structural; the counter state lives in the DB as Invoice Number Sequence — changing the pattern is a rare, deliberate event). Syntax: literal text plus `{year}`, `{month}` (optional), `{counter}`/`{counter:N}` (zero-padded), e.g. `{year}-{counter:4}` → 2026-0042. The counter scope derives from the pattern's date parts ({year} → yearly reset; no date parts → one global counter); pattern validated at startup (must contain {counter})
* currency (effectively constant; changing it mid-data would corrupt the meaning of stored prices)
* default country (ISO 3166-1 alpha-2) used when a customer or the practice has none of its own.
  Deliberately not a database default: it is a business preference, so the columns stay nullable
  and this fills in. Validated against the real ISO 3166-1 list at startup, not by shape.
* payment term in days. Pinned onto each invoice as its due date at creation, so changing the
  term never moves the due date of an invoice already issued.
* VAT normal german rate: 19%, reduced: 7% (set by law; these only feed the VAT selector for new drugs/services — actual rates are stored per Drug/Service and pinned per Treatment Item)
* outbound SMTP server (infrastructure, includes credentials)
* base url
* listen address and port (infrastructure)
* log level (infrastructure; runtime override via environment possible)
* invoice template (Typst; default embedded in the binary, optional file-path override in config, versioned in git)
* invoice email template (file next to the config; Jinja syntax via minijinja, plain text UTF-8, first line = subject template; default embedded in the binary, the file overrides)
* user and password (see general guidelines)
