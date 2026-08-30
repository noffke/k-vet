// Default invoice template (Typst).
//
// The document is always German (spec FR-030): every number, date and amount arrives
// pre-formatted de-DE from the application, so this template only places text. It never
// computes — in particular it never adds VAT. The gross amounts it prints already reconcile with
// the VAT summary, because VAT is rounded per line and summed (see `money::vat_summary`).
//
// Override it with `[invoice] typst_template = "/path/to/invoice.typ"` in the config.
// See docs/templates.md for the full data reference.

#import sys: inputs

#let data = json(bytes(inputs.data))
#let practice = data.practice
#let invoice = data.invoice

// Colours lifted from the practice's website.
#let rust = rgb("#9b5023")
#let muted = rgb("#52658a")
#let rule = rgb("#cdbf9f")
#let band = rgb("#f4eedf")

// Joins the parts that are actually filled — empty settings must not leave stray separators.
#let joined(parts, sep: " · ") = parts.filter(part => part != none and part != "").join(sep)

#set page(
  paper: "a4",
  margin: (left: 25mm, right: 20mm, top: 15mm, bottom: 32mm),
  header: [
    #if practice.logo_present [
      #align(right)[#image(bytes(inputs.logo), height: 22mm)]
    ]
  ],
  header-ascent: 6mm,
  footer: context [
    #set text(7.5pt, fill: muted)
    #align(center)[
      #joined((practice.name, practice.address_line, practice.email)) \
      #joined((
        practice.bank_name,
        if practice.iban != "" { "IBAN: " + practice.iban },
        if practice.bic != "" { "BIC: " + practice.bic },
      )) \
      #joined((
        if practice.ustid != "" { "USt-IdNr: " + practice.ustid },
        "Seite " + str(counter(page).display()) + " von "
          + str(counter(page).final().first()),
      ))
    ]
  ],
  footer-descent: 8mm,
)
// Source Sans 3 travels in the binary (see `pdf::SANS_FACES`); the serif faces
// typst-assets bundles stay available to a custom template.
#set text(font: ("Source Sans 3",), size: 10pt, lang: "de")

// ── Anschriftenfeld (DIN 5008) ────────────────────────────────────────────────
#v(4mm)
#text(7.5pt, fill: muted)[#invoice.sender_line]
#v(2mm)
#block[
  #set par(leading: 0.55em)
  #for line in invoice.recipient [#line \ ]
]

#v(10mm)

// ── Rechnungskopf ─────────────────────────────────────────────────────────────
#text(15pt, weight: "bold", fill: rust)[Rechnung]
#v(3mm)
#block[
  #set text(10pt)
  #set par(leading: 0.65em)
  Rechnungs-Nr: #invoice.number \
  Rechnungsdatum: #invoice.date
  // Its own labelled line, next to the other dates: this is the form the bookkeeping software's
  // document recognition reads, so the vet stops typing the due date in by hand.
  #if invoice.due_date != "" [\ Fälligkeitsdatum: #invoice.due_date]
]

#v(8mm)

#if invoice.greeting != "" [
  #invoice.greeting,
  #v(3mm)
]

Vielen Dank für Ihr Vertrauen!
#if invoice.due_date != "" [
  Bitte überweisen Sie den Rechnungsbetrag bis zum #invoice.due_date.
] else [
  Bitte überweisen Sie den Rechnungsbetrag ohne Abzug.
]

#v(6mm)

// ── Positionen, nach Tier gruppiert ───────────────────────────────────────────
#{
  let rows = ()
  for group in invoice.patient_groups {
    if group.patient != "" {
      rows.push(table.cell(colspan: 5, inset: (top: 8pt, bottom: 2pt))[
        #text(weight: "bold")[Tier: #group.patient]
      ])
      if group.description != "" {
        rows.push(table.cell(colspan: 5, inset: (top: 0pt, bottom: 2pt))[
          #text(9pt, fill: muted)[#group.description]
        ])
      }
    }
    if group.service_date != "" {
      rows.push(table.cell(colspan: 5, inset: (top: 2pt, bottom: 3pt))[
        #text(9pt, style: "italic")[Leistungsdatum: #group.service_date]
      ])
    }
    for item in group.items {
      rows.push([
        #item.name
        #if item.detail != "" [\ #text(8pt, fill: muted)[#item.detail]]
        #if item.factor != "" and item.factor != "100 %" [
          \ #text(8pt, fill: muted)[Faktor: #item.factor]
        ]
        #if item.km != "" [\ #text(8pt, fill: muted)[#item.km km]]
      ])
      rows.push([#item.quantity])
      rows.push([#item.vat])
      rows.push([#item.price])
      rows.push([#item.total])
    }
  }

  table(
    columns: (1fr, auto, auto, auto, auto),
    align: (left, right, right, right, right),
    stroke: none,
    inset: (x: 4pt, y: 4pt),
    fill: (_, row) => if row == 0 { band },
    table.header(
      [*Artikel/Leistung*], [*Menge*], [*MwSt*], [*Einzelpreis*], [*Gesamt*],
    ),
    ..rows,
  )
}

#line(length: 100%, stroke: 0.5pt + rule)
#v(3mm)

// ── Steuerübersicht und Summe ─────────────────────────────────────────────────
#grid(
  columns: (auto, 1fr),
  align: (left + top, right + top),
  [
    #set text(9pt)
    #table(
      columns: 4,
      stroke: none,
      inset: (x: 5pt, y: 2pt),
      align: (right, right, right, right),
      [*%*], [*Netto*], [*MwSt*], [*Brutto*],
      ..invoice.vat_groups.map(group => (
        [#group.rate], [#group.net], [#group.vat], [#group.gross],
      )).flatten(),
    )
  ],
  [
    #table(
      columns: 2,
      stroke: none,
      inset: (x: 5pt, y: 3pt),
      align: (left, right),
      [*Gesamtsumme*], [*#invoice.total*],
    )
  ],
)

#if invoice.note != "" [
  #v(5mm)
  #text(10pt)[#invoice.note]
]

// ── Behandlungsbericht ────────────────────────────────────────────────────────
// One block per animal. Animals with neither a reason nor a finding are not in `reports`.
#if invoice.reports.len() > 0 [
  #v(8mm)
  #if invoice.treatment_heading != "" [
    #text(10pt, weight: "bold")[#invoice.treatment_heading]
    #v(2mm)
  ]
  #set text(9.5pt)
  #set par(leading: 0.65em)
  #for report in invoice.reports [
    #text(weight: "bold")[#report.patient]
    #if report.description != "" [ #text(fill: muted)[(#report.description)]]
    #v(1mm)
    #if report.treatment_reason != "" [
      *Vorbericht und Untersuchung:* #report.treatment_reason
      #v(1mm)
    ]
    #if report.finding != "" [
      *Therapie und weiteres Vorgehen:* #report.finding
    ]
    #v(3mm)
  ]
]

// ── GiroCode ──────────────────────────────────────────────────────────────────
// Last in the document: the report is what the customer reads through, the payment details are
// what they act on once they have. `breakable: false` keeps the code together with the sentence
// that explains it, and moves the pair to the next page only when they do not fit on this one —
// no explicit page break, which would strand the block on a sheet of its own every time.
#if invoice.qr_present [
  #v(8mm)
  #block(breakable: false)[
    #grid(
      columns: (auto, 1fr),
      column-gutter: 4mm,
      align: (left + horizon, left + horizon),
      image(bytes(inputs.qr), format: "svg", width: 23mm),
      [
        #set text(8.5pt)
        #text(weight: "bold")[Bequem per GiroCode bezahlen] \
        #text(fill: muted)[
          Code mit der Banking-App scannen — Empfänger, IBAN, Betrag und
          Verwendungszweck sind dann bereits ausgefüllt.
        ] \
        #invoice.total · Verwendungszweck: Rechnung #invoice.number
      ],
    )
  ]
]
