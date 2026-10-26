# Templates

Two documents leave the practice: the invoice PDF and the email that carries it. Both come from
templates, so the layout and wording can change without a new release. The PDF template ships
inside the binary and can be replaced with a file; the email templates are files in the
**templates folder** — `/etc/k-vet/templates` in the container, where the entrypoint seeds
`invoice-email.txt` on first start, and `$KVET_TEMPLATES_DIR` elsewhere.

```toml
[invoice]
typst_template = "/etc/k-vet/templates/invoice.typ"   # empty = embedded default

# The email templates to choose from, in this order — the first is preselected.
[[invoice.email_templates]]
name = "Standard"             # what the vet sees when releasing or sending
file = "invoice-email.txt"    # a file in the templates folder, not a path

[[invoice.email_templates]]
name = "Kurz"
file = "invoice-email-kurz.txt"
```

Both kinds are **read when used** — the PDF template on every render, an email template on
every send — so an edit takes effect with the next invoice or mail. Listing a *new* email
template is a change to `config.toml`, which is read at startup: restart for that. A template
that fails to render fails that one invoice or mail and says so (the PDF with a 500; a mail is
given up on, with the reason, on the dashboard); the invoice keeps its number and can be rendered
or sent again after the fix. Test a change on a throwaway invoice before billing a
customer with it.

Everything reaching a template is **already formatted in German conventions** (`1.234,56 €`,
`04.05.2026`). Templates place text; they never do arithmetic or turn numbers and dates into
text. That keeps the money rules — AMPreisV, GOT factors, VAT per rate — in one place, where they
are unit-tested. The one formatting a template does apply is the vet's own: the bold, italic,
underline and lists of the reason and the finding arrive spelt out, for the template to lay out
(see *Formatted text* below).

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

Only the fonts embedded in the binary are available: `Source Sans 3` (regular, bold, italic and
bold italic — what the default template sets in), plus `Libertinus Serif`, `New Computer Modern`,
`New Computer Modern Math` and `DejaVu Sans Mono`. System fonts are deliberately not searched — the
appliance has none installed, and a template must render identically everywhere. Naming any other
font falls back silently and changes the metrics, so stay with these.

Source Sans 3 is the only one that does not come from `typst-assets`: its four faces live in
`k-vet-backend/fonts/` (SIL Open Font License 1.1) and are compiled into the binary. The serif
faces stay available, so a template that prefers them only has to say so.

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
| `treatment_heading` | string | e.g. `Behandlung/Konsultation Eddie (Hund – Havaneser) am 28.07.2026`; empty when there are no animals. |
| `reports` | report[] | The clinical report, one entry per animal. See below. |
| `patient_groups` | group[] | The lines grouped per animal — what the default template prints. See below. |
| `items` | line[] | Every line in one flat list, in the vet's chosen order. Kept so templates written before grouping keep working. |
| `vat_groups` | group[] | One entry per VAT rate on the invoice. |
| `total` | string | Gross total, e.g. `38,62 €`. |
| `note` | string | The internal note from the create dialog; may be empty. **The default template does not print it** — it is written for the practice, not for the customer, and the field is labelled *Notiz (intern)* on screen. Still passed so a template can use it for a copy kept in-house; do not put it on the customer's invoice. |
| `qr_present` | bool | `true` when `inputs.qr` holds a GiroCode. Guard the QR block with this. |

A report (`invoice.reports[]`) — the Behandlungsbericht, per animal. An animal with neither a
reason nor a finding is **not** in the list, so `reports.len() > 0` is the guard for the whole
section:

| Field | Notes |
| --- | --- |
| `patient` | Animal name. |
| `description` | e.g. `Hund, Havaneser, Geburtsdatum: 01.01.2021, Chipnummer: 276098106222333`; empty parts are left out. |
| `treatment_reason_blocks` | Why the animal was seen and what the examination showed, **formatted** — printed under *Vorbericht und Untersuchung*. Blocks as described below; empty when nothing was written. The wire name predates that heading. |
| `finding_blocks` | What was treated and what happens next, **formatted** — printed under *Therapie und weiteres Vorgehen*. Only filled when the invoice was created with that box ticked; otherwise empty. |
| `treatment_reason` | The same reason as **plain text**: a line per paragraph and per list item (`• …`, `1. …`), formatting dropped; may be empty. Kept for templates written before formatting. |
| `finding` | The same finding as plain text, on the same terms. |

#### Formatted text

The reason and the finding carry the vet's formatting — bold, italic, underline, bullet and
numbered lists one level deep. In `*_blocks` they arrive as a list of blocks, each a paragraph or a
list, made of **spans**: runs of text with their formatting spelt out.

| Block | Fields |
| --- | --- |
| `kind: "paragraph"` | `spans` — the paragraph's runs. Empty for a blank line the vet left between paragraphs; blank lines around the text are already dropped. |
| `kind: "list"` | `numbered` (bool), `start` (the first number, for a numbered list), `items` — one array of spans per item. |

A span is `text` plus `bold`, `italic` and `underline` (bools). A line break inside a paragraph is
a span whose text is `"\n"` (or contains one).

The default template lays them out with two helpers — copy them into a custom template rather than
writing your own:

```typst
#let span-content(span) = {
  let body = span.text.split("\n").map(part => [#part]).join(linebreak())
  if span.underline { body = underline(body) }
  if span.italic { body = emph(body) }
  if span.bold { body = strong(body) }
  body
}
#let spans-content(spans) = spans.map(span-content).join()
```

`formatted(label, blocks)` in the default template builds on them: the label leads the first
paragraph (or stands on a line of its own above a leading list), lists become `list` and
`enum(start: …)`. Place `span.text` as content, as above — never `eval` it: it is the vet's
text, and evaluated it would act as Typst markup.

> **Changed with formatted text.** A template written before the reason and the finding could be
> formatted keeps working: `treatment_reason` and `finding` are still strings, now the plain text
> of the formatted ones. It prints them **without** the formatting. To print bold, italic,
> underline and lists, take the report section and the helpers above from the current default
> template — in a container, a copy seeded into `/etc/k-vet/templates` on an earlier start is not
> updated by the entrypoint.

> **Changed with the Patientenbehandlung.** The reason and the finding used to be single
> invoice-level fields, `invoice.treatment_reason` and `invoice.finding`, because a treatment
> had one of each for all its animals. They are now recorded per animal and only exist inside
> `reports`. A custom template that printed the old fields has to iterate instead.

A patient group (`invoice.patient_groups[]`):

| Field | Notes |
| --- | --- |
| `patient` | Animal name. **Empty** for the visit's own positions — services marked "pro Termin", such as the house-call fee or Wegegeld — print those without a heading. Such a group always comes last. |
| `description` | e.g. `Hund, Havaneser, Geburtsdatum: 01.01.2021, Chipnummer: 276098106222333`; empty parts are left out. |
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
| `got_number` | GOT position number; empty for drugs and for self-defined services that name none. |
| `km` | Kilometres on a travel-expense line; empty otherwise. |
| `vat` | This line's VAT rate, e.g. `19 %`. |
| `detail` | The small second line: `GOT-Nr. 16`, `GOT-Nr. 16 (§8)` for a self-defined position charged analogously to that GOT number, `1 Stück`, or `1 Originalpackung, Zulassungsnr: 402485.00.00`. Empty when there is nothing to add. |
| `price` | Unit price, gross, **with the GOT factor already applied** — so `price × quantity` reconciles with `total`. Not the bare catalogue fee; that is the net figure the vet edits. |
| `total` | Line total, gross. |

A VAT group (`invoice.vat_groups[]`) has `rate` (e.g. `19 %`), `net`, `vat` and `gross`. Sum the
groups rather than recomputing anything: prices are stored **net**, VAT is computed per group, and
`total` is the authoritative gross amount.

**Never derive a gross amount yourself.** VAT is rounded per line and summed, so `total` is that
line's `net + vat` and the column adds up to the group's `gross` exactly. Multiplying a group total
by hand produces a figure a cent or so off, because the group's VAT is the sum of its lines' VAT
rather than the rate applied to the group's net.

Empty strings are the "absent" signal throughout — there are no nulls. Guard optional fields:

```typst
#for report in invoice.reports [
  #if report.finding != "" [
    #text(10pt)[*Therapie und weiteres Vorgehen:* #report.finding]
  ]
]
```

#### Page breaks

A heading must not end a page apart from what it heads. The default template keeps them together
in two ways — worth copying into a custom template:

- `block(sticky: true)[…]` around the treatment heading, each animal's name line in the report,
  and a label that stands above a list (inside `formatted`). A sticky block moves to the next
  page together with whatever follows it.
- In the positions table, each animal's rows (Tier, description, Leistungsdatum) are a
  subheader, `table.header(level: 2, repeat: false, …)`, so they move with the animal's first
  position.

(Mind Typst's scoping: a loop variable called `block` hides the `block` function.)

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

The template the container seeds as "Standard" is
[`k-vet-backend/templates/invoice-email.txt`](../k-vet-backend/templates/invoice-email.txt).

### Several templates

Each `[[invoice.email_templates]]` entry is a template the vet can choose when releasing an
invoice or sending it again; the first listed is preselected, and a waiting mail reopens on its
own. To add one, copy `invoice-email.txt` in the templates folder under a new name, change the
text, list it with a name in `config.toml`, and restart.

- **A missing file does not stop k-vet.** The template is still offered, disabled, with the file
  name that is missing; once the file is put in place it can be chosen — no restart.
- **A queued mail keeps its template's name.** If that template is no longer listed, or its file
  is gone when the mail is due, the mail is not sent with another text: it is given up on with
  the reason, on the dashboard and on the appointment, to be retried once the file is back or
  sent again with another template.
- **The invoice records which template each delivery used**, next to the addresses it went to
  ("An … · Vorlage: Kurz"); a resend records its own.
- **`file` is a file name**, not a path: every email template lives in the one folder.
- Without any `[[invoice.email_templates]]` there is one template, "Standard", from
  `invoice-email.txt`. The former single key `email_template` is still accepted so an older
  `config.toml` starts, but it is no longer read — the start-up log says so.

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
