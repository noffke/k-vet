# Templates

Two documents leave the practice: the invoice PDF and the email that carries it. Both come from
templates that ship inside the binary and can be replaced with a file, so the layout and wording
can change without a new release.

```toml
[invoice]
typst_template = "/etc/k-vet/invoice.typ"        # empty = embedded default
email_template = "/etc/k-vet/invoice-email.txt"  # empty = embedded default
```

The **PDF template is read on every render**, so an edit takes effect with the next invoice. The
**email template is read once at startup**, so restart the service after changing it. A template
that fails fails that one request with a 500 and logs the error; the invoice keeps its number and
can be rendered or sent again after the fix. Test a change on a throwaway invoice before billing a
customer with it.

Everything reaching a template is **already formatted in German conventions** (`1.234,56 €`,
`04.05.2026`). Templates place text; they never do arithmetic or formatting. That keeps the money
rules — AMPreisV, GOT factors, VAT per rate — in one place, where they are unit-tested.

---

## Invoice PDF (Typst)

The default template is [`k-vet-backend/templates/invoice.typ`](../k-vet-backend/templates/invoice.typ).
Start from a copy of it.

### How the data arrives

Typst is called with two system inputs:

| Input | Contents |
| --- | --- |
| `inputs.data` | The whole invoice as a JSON string. |
| `inputs.logo` | The practice logo's bytes, empty when none is set. |
| `inputs.qr` | The GiroCode as SVG bytes, empty when the invoice cannot carry one. Guard with `invoice.qr_present` rather than testing the bytes. |

```typst
#import sys: inputs
#let data = json(bytes(inputs.data))
#let practice = data.practice
#let invoice = data.invoice
```

Only the fonts embedded in the binary are available: `Libertinus Serif`, `New Computer Modern`,
`New Computer Modern Math` and `DejaVu Sans Mono`. System fonts are deliberately not searched — the
appliance has none installed, and a template must render identically everywhere. Naming any other
font falls back silently and changes the metrics, so stay with these.

### `practice`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string | From *Einstellungen*; may be empty. |
| `address` | string | Street and town, one per line. |
| `address_line` | string | The same address joined with `, ` — for a footer or sender line. |
| `email` | string | Practice address for the footer; empty when not set. |
| `iban` | string | Empty when not set — guard before printing a payment sentence. |
| `bic` | string | Empty when not set. |
| `bank_name` | string | Empty when not set. |
| `ustid` | string | VAT ID, empty when not set. |
| `logo_present` | bool | `true` when `inputs.logo` holds an image. |

### `invoice`

| Field | Type | Notes |
| --- | --- | --- |
| `number` | string | e.g. `2026-0042`. |
| `date` | string | Invoice date, `dd.mm.yyyy`. |
| `due_date` | string | Pinned at creation from `[invoice] payment_terms_days`; empty on invoices written before that existed. |
| `treatment_date` | string | Appointment date; empty when unknown. |
| `recipient` | string[] | Address block, one line each: optional company, then name(s), then street, then zip and city. Already picks the invoice address over the home address, and includes a second household name when there is one. |
| `sender_line` | string | Practice and address on one line, for the DIN 5008 Rücksendeangabe above the address block. |
| `greeting` | string | Ready-made salutation, e.g. `Sehr geehrter Herr Mustermann` — follows the invoice recipient when one is set. Write the comma yourself. |
| `patients` | string[] | Animal names on this invoice. |
| `treatment_reason` | string | May be empty. |
| `treatment_heading` | string | e.g. `Behandlung/Konsultation Eddie (Hund – Havaneser) am 28.07.2026`; empty when there are no animals. |
| `finding` | string | Only filled when the invoice was created with *Befund aufführen*; otherwise empty. |
| `patient_groups` | group[] | The lines grouped per animal — what the default template prints. See below. |
| `items` | line[] | Every line in one flat list, in the vet's chosen order. Kept so templates written before grouping keep working. |
| `vat_groups` | group[] | One entry per VAT rate on the invoice. |
| `total` | string | Gross total, e.g. `38,62 €`. |
| `note` | string | Free note from the create dialog; may be empty. |
| `qr_present` | bool | `true` when `inputs.qr` holds a GiroCode. Guard the QR block with this. |

A patient group (`invoice.patient_groups[]`):

| Field | Notes |
| --- | --- |
| `patient` | Animal name. **Empty** for lines the vet did not attribute to an animal — print those without a heading. Such a group always comes last. |
| `description` | e.g. `Hund, Havaneser, Geburtsdatum: 01.01.2021`; empty parts are left out. |
| `service_date` | Treatment date, repeated per group. |
| `items` | The group's lines, in the vet's order. |

A line (`invoice.items[]`):

| Field | Notes |
| --- | --- |
| `position` | 1-based line number. |
| `name` | Drug or service name as pinned when the line was added. |
| `patient` | Animal name — only filled when the invoice covers more than one animal. |
| `quantity` | Number of packagings or units, German decimals. |
| `unit` | e.g. `ml`; empty for services. |
| `factor` | GOT factor as a percentage (`150 %`); empty when none applies. |
| `got_number` | GOT position number; empty for self-defined services and drugs. |
| `km` | Kilometres on a travel-expense line; empty otherwise. |
| `vat` | This line's VAT rate, e.g. `19 %`. |
| `detail` | The small second line: `GOT-Nr: 16`, `1 Stück`, or `1 Originalpackung, Zulassungsnr: 402485.00.00`. Empty when there is nothing to add. |
| `price` | Unit price, gross. |
| `total` | Line total, gross. |

A VAT group (`invoice.vat_groups[]`) has `rate` (e.g. `19 %`), `net`, `vat` and `gross`. Sum the
groups rather than recomputing anything: prices are stored **net**, VAT is computed per group, and
`total` is the authoritative gross amount.

**Never derive a gross amount yourself.** `price` and `total` are not simply `net × 1,19`: the odd
cent is allocated across a group's lines so the printed column adds up to `gross` exactly
(`money::allocate_gross`). Multiplying by hand produces a column that is a cent off its own total.

Empty strings are the "absent" signal throughout — there are no nulls. Guard optional fields:

```typst
#if invoice.finding != "" [ #text(10pt)[*Befund:* #invoice.finding] ]
```

### Example: a minimal template

```typst
#import sys: inputs
#let data = json(bytes(inputs.data))
#let invoice = data.invoice

#set page(paper: "a4", margin: 20mm)
#set text(font: ("Libertinus Serif",), size: 10.5pt, lang: "de")

#text(13pt, weight: "bold")[#data.practice.name]
#v(6mm)
#for line in invoice.recipient [#line \ ]
#v(8mm)

= Rechnung #invoice.number
Rechnungsdatum: #invoice.date

#table(
  columns: (auto, 1fr, auto, auto),
  table.header([*Pos.*], [*Bezeichnung*], [*Menge*], [*Gesamt*]),
  ..invoice.items.map(item => (
    [#item.position], [#item.name], [#item.quantity], [#item.total],
  )).flatten(),
)

#align(right)[*Gesamtbetrag: #invoice.total*]
```

---

## Invoice email (minijinja)

The default template is
[`k-vet-backend/templates/invoice-email.txt`](../k-vet-backend/templates/invoice-email.txt).

### Structure: first line is the subject

```
Ihre Rechnung {{ invoice_number }}
{{ greeting }},

vielen Dank für Ihren Besuch. Anbei erhalten Sie die Rechnung {{ invoice_number }} …
```

The file is split at the **first newline**: line 1 is the subject template, everything after it
(leading blank lines removed) is the body. Both are rendered with
[minijinja](https://docs.rs/minijinja) — Jinja2 syntax: `{{ value }}`, `{% if %}`, `{% for %}`,
filters like `join`.

The mail is plain text only. There is no HTML alternative, deliberately: it reads the same in
every client, and the invoice itself is the attached PDF (named `Rechnung-<number>.pdf`).

### Variables

| Variable | Notes |
| --- | --- |
| `greeting` | Ready-made salutation line, e.g. `Sehr geehrte Frau Mustermann`, and `Sehr geehrte Frau Mustermann, sehr geehrter Herr Mustermann` for a two-name household. Write the comma yourself. |
| `salutation` | `Frau`, `Herr` or `Familie`; empty when unknown. |
| `first_name`, `last_name` | Customer name parts. |
| `second_salutation`, `second_first_name`, `second_last_name` | The second name of the household; empty when there is none. |
| `invoice_number` | e.g. `2026-0042`. |
| `invoice_date` | `04.05.2026`. |
| `invoice_total` | `38,62 €`. |
| `practice_name` | From *Einstellungen*. |
| `patients` | List of animal names — join them with the `join` filter, and guard with `{% if patients %}` (see the default template). |

Undefined variables render as empty rather than failing, so a typo shows up as a gap in the
text — read the result before sending.

### Encoding and umlauts

Save the file as **UTF-8 without BOM**. Umlauts and `ß` are fine anywhere, in the body and in the
subject: the subject is encoded as an RFC 2047 encoded word and the body is sent as UTF-8
`text/plain`, both by lettre. Do not try to escape or transliterate them (`ue` for `ü`) — that
only makes the German look wrong.

Keep lines reasonably short (below ~78 characters) so quoted replies stay readable in every mail
client.

---

## Testing a template change

```bash
# Backend tests cover both renderers, including a German address block and umlaut subjects.
cd k-vet-backend && cargo test --test invoices && cargo test --lib mail::

# End to end: the E2E suite generates a real invoice and reads the PDF's text back with
# pdftotext, asserting the practice's IBAN and name are on it.
cd ../e2e && npx playwright test dashboard-settings
```

For a manual check, point the config at your template, create an invoice on a test customer and
open its PDF from the invoice list — the same bytes the customer receives.
