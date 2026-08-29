# Future Ideas
*Not to be implemented now!*
* appointment date sanity check: if more than 1 week in future or past show confirmation dialog. make configurable and allow to disable alltogether
* cloud form for customer self registration
* send AGB, DSGVO via email
* integrate google contacts
* integrate google maps link (start navigation to address)
* phone call integration (start phone call from address)
* inventory (Inventur) support: list current stock, allow to tick off or correct stock of pharmaceuticals
* scan EAN, Chargennummer, exp. date of inbound pharmaceuticals
* scan chip no barcode
* easy upload of files for pets from mobile (scan qr-code?)
* for travel expenses service, calculate driving distance based on address 
* vaccination reminders
* target invoice calculation: set desired end price and then calculate the factor to get close to the end price
* price information: allow to build a non-persistent list of drugs and services to answer the customer the "what would it cost me" question
* delayed invoice email sending: enter date and time and invoice email will be sent when scheduled 
* list prices from Barsoi (barsoiliste.de). Researched 2026-08-03: there is an XML "Preisliste zum
  Einlesen in Verwaltungs- und Bestellsysteme", 12 € per month plus VAT, carrying
  "Medikamentennamen, Hersteller, Handelsformen, Einkaufs- und Verkaufspreise" with the prices
  "kalkuliert nach Arzneimittelpreisverordnung § 3, 4 oder 10" and the 7 %/19 % VAT rate. An import
  would fill `list_price_net` and `vat_percent`; because Barsoi computes § 3/4/10 itself, it would
  also hand us an independent cross-check of our own `sales_price_net`, which is worth having for
  SC-005. One purchase price per product, with no wholesale/manufacturer split — so nothing extra
  to reconcile. All of this is from the product description; nobody has seen the actual XML.
* de-DE end-user manual (Benutzerhandbuch) for the daily workflows (treatment, invoice, stock)
## E-Rechnung (ZUGFeRD / XRechnung)

Researched 2026-07-30; deliberately **not** built yet. Recorded so the reasoning is not lost.

**Why it is tempting now, independently of any legal deadline:** the vet uploads every invoice to
lexoffice and sets the due date by hand each time. Lexware Office pre-fills its Erfassungsmaske from
the machine-readable part of an e-invoice — *"Die Daten, welche im rechten Erfassungsbereich
eingesetzt sind, stammen bei korrekt ausgestellten E-Rechnungen aus dem maschinenlesbaren Teil der
Datei."* It accepts XRechnung 3.0.1 (CII and UBL), ZUGFeRD 2.0+ and ZUGFeRD 2.2.0 Profil XRechnung.
The help article does not enumerate the pre-filled fields, so it is **unconfirmed** that the due
date specifically is among them.

**Current step instead:** the due date is printed as its own labelled line (`Fälligkeitsdatum:`)
next to the other dates, where document recognition expects it. Measure that first — item `OFF-01`
in `review.md`. If it is not picked up, build the XML.

**When building it, prefer the XML sidecar.** lexoffice accepts e-invoices as PDF *or* XML, so a
standalone XRechnung/CII file delivers the same benefit without the hybrid-PDF machinery. The
container itself is nearly free (`typst-pdf 0.15.1` has `PdfStandard::A_3b`, `typst-library 0.15.1`
has `pdf.attach(relationship: "alternative")`, whose own docs cite ZUGFeRD/Factur-X), but the XMP
extension schema is not: krilla has the machinery at `configure/validate.rs:961` and it is
`pub(crate)`, with no hook exposed by Typst. That would mean patching the uncompressed `/Metadata`
packet after every render, re-verified on each Typst bump — or a PR upstream.

**Still missing for EN 16931 after the work of 2026-07-30:** UN/ECE Rec 20 unit codes (k-vet has a
free-text `unit`). Structured addresses, country codes, the due date and net-first line amounts are
all in place now.

**Rejected for now:** the lexoffice REST API (a `salesinvoice` voucher with an explicit `dueDate`
plus a file upload). Deterministic and about a day's work, but it adds an API key, network egress
from the Pi, coupling to one vendor, and automatic transmission of customer data to an external
service.

**Legal timing:** § 14 UStG has obliged German businesses to *receive* e-invoices since
2025-01-01 (an inbox concern, not this application). *Issuing* is **B2B only** — from 2027-01-01
above €800k prior-year turnover, from **2028-01-01** for everyone else. **B2C is exempt.** A
single-vet practice billing pet owners is B2C, so only the occasional business customer (a breeder,
a shelter, a farm) is ever in scope, from 2028-01-01.
