// Default invoice template (Typst).
//
// The document is always German (spec FR-030): every number, date and amount arrives
// pre-formatted de-DE from the application, so this template only places text.
//
// Override it with `[invoice] typst_template = "/path/to/invoice.typ"` in the config.
// See docs/templates.md for the full data reference.

#import sys: inputs

#let data = json(bytes(inputs.data))
#let practice = data.practice
#let invoice = data.invoice

#set page(
  paper: "a4",
  margin: (left: 25mm, right: 20mm, top: 20mm, bottom: 25mm),
  footer: context [
    #set text(8pt, fill: rgb("#52658a"))
    #grid(
      columns: (1fr, auto),
      align: (left, right),
      [#practice.name #if practice.ustid != "" [· USt-IdNr. #practice.ustid]],
      [Seite #counter(page).display() von #counter(page).final().first()],
    )
  ],
)
#set text(font: ("Libertinus Serif",), size: 10.5pt, lang: "de")
#show heading: set text(fill: rgb("#9b5023"))

// ── Practice head ──────────────────────────────────────────────────────────────
#grid(
  columns: (1fr, auto),
  align: (left + top, right + top),
  [
    #if practice.logo_present [
      #image(bytes(inputs.logo), height: 18mm)
      #v(2mm)
    ]
    #text(13pt, weight: "bold", fill: rgb("#9b5023"))[#practice.name]
  ],
  [
    #set text(9pt)
    #set par(leading: 0.5em)
    #practice.address
  ],
)

#v(8mm)

// ── Recipient and invoice metadata ─────────────────────────────────────────────
#grid(
  columns: (1fr, auto),
  align: (left + top, right + top),
  [
    #set par(leading: 0.55em)
    #for line in invoice.recipient [#line \ ]
  ],
  [
    #set text(9.5pt)
    #table(
      columns: 2,
      stroke: none,
      inset: (x: 3pt, y: 2pt),
      column-gutter: 6pt,
      align: (left, right),
      [Rechnungsnummer], [#invoice.number],
      [Rechnungsdatum], [#invoice.date],
      ..if invoice.treatment_date != "" { ([Behandlungsdatum], [#invoice.treatment_date]) } else { () },
    )
  ],
)

#v(10mm)

= Rechnung #invoice.number

#if invoice.patients.len() > 0 [
  #v(1mm)
  #text(10pt)[*Patient#if invoice.patients.len() > 1 [en]:* #invoice.patients.join(", ")]
]

#if invoice.treatment_reason != "" [
  #v(1mm)
  #text(10pt)[*Behandlungsgrund:* #invoice.treatment_reason]
]

#if invoice.finding != "" [
  #v(1mm)
  #text(10pt)[*Befund:* #invoice.finding]
]

#v(6mm)

// ── Line items ────────────────────────────────────────────────────────────────
#table(
  columns: (auto, 1fr, auto, auto, auto, auto),
  align: (right, left, right, right, right, right),
  stroke: none,
  inset: (x: 4pt, y: 5pt),
  fill: (_, row) => if row == 0 { rgb("#f4eedf") },
  table.header(
    [*Pos.*], [*Bezeichnung*], [*Menge*], [*Faktor*], [*Einzelpreis*], [*Gesamt*],
  ),
  ..invoice.items.map(item => (
    [#item.position],
    [
      #item.name
      #if item.got_number != "" [ #text(8.5pt, fill: rgb("#52658a"))[(GOT #item.got_number)]]
      #if item.patient != "" [\ #text(8.5pt, fill: rgb("#52658a"))[#item.patient]]
      #if item.km != "" [\ #text(8.5pt, fill: rgb("#52658a"))[#item.km km]]
    ],
    [#item.quantity#if item.unit != "" [ #item.unit]],
    [#item.factor],
    [#item.price],
    [#item.total],
  )).flatten(),
  table.hline(stroke: 0.5pt + rgb("#cdbf9f")),
)

#v(4mm)

// ── Totals and VAT summary ────────────────────────────────────────────────────
#align(right)[
  #table(
    columns: (auto, auto),
    stroke: none,
    align: (left, right),
    inset: (x: 4pt, y: 3pt),
    ..invoice.vat_groups.map(group => (
      [Netto #group.rate],
      [#group.net],
    )).flatten(),
    ..invoice.vat_groups.map(group => (
      [USt. #group.rate],
      [#group.vat],
    )).flatten(),
    table.hline(stroke: 0.5pt + rgb("#cdbf9f")),
    [*Gesamtbetrag*], [*#invoice.total*],
  )
]

#v(8mm)

#if invoice.note != "" [
  #text(10pt)[#invoice.note]
  #v(4mm)
]

#text(9.5pt)[
  Zahlbar ohne Abzug.
  #if practice.iban != "" [ Bitte überweisen Sie den Betrag auf das Konto #practice.iban.]
]
