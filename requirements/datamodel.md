# The Datamodel
Generally, all entities should have a created and updated timestamp.

## Modeling Conventions
* Entities with variants get an explicit discriminator column (enum type) — the variant is declared, never inferred from which columns happen to be NULL. CHECK constraints tie variant-specific columns to the declared type.
* XOR relations ("1 A XOR 1 B") follow the same convention: one table, a kind enum column, two nullable FKs, CHECK constraints binding each FK to its kind.
* In the application, variants map to a Rust enum and are exposed in the API as a discriminated (tagged) union.
* Master data (Customer, Patient, Drug, Drug Packaging, Service, Supplier, Manufacturer, Treatment Template) is archived (soft delete via Archived flag), never hard-deleted once referenced by invoiced treatments. Patients with a Date of Death are archived automatically.
* Addresses are structured per German conventions, stored as a column group on the owning entity: Addon (Adresszusatz, optional, e.g. "c/o Fr. Müller"), Street incl. house number (not split out), ZIP (PLZ), City. An optional address is all-or-none (CHECK): street/ZIP/city set together or not at all.
* Contact data is validated: e-mail addresses get standard syntax validation; phone numbers are parsed libphonenumber-style with default region DE (international input allowed), stored normalized as E.164, and displayed in national format. Validation is defined once in the backend and mirrored into the UI via the generated schemas.
* Person names are structured: Salutation (enum: Frau | Herr | Familie), First Name (optional), Last Name. en-US term is last_name (not surname).
* Draft rows (auto-save): entities edited via forms are created immediately as draft = true so every keystroke has an ID to save against. Mandatory fields are enforced as CHECK "draft OR field IS NOT NULL" instead of plain NOT NULL; the server recomputes the flag on every write and flips it to false automatically with the write that completes the mandatory set — there is no save button. The transition is one-way: clearing a mandatory field on a complete record is rejected as a validation error. Drafts are hidden from pickers and billing, shown as "incomplete" in lists (resumable); abandoned all-empty drafts are cleaned up automatically. Applies to master data and appointments; treatments, treatment items, stock movements, and invoices are born complete.

## Entities
### Pharmacy (Apotheke)
* Supplier (Lieferant)
  * Name
  * Address (structured, optional, see Modeling Conventions)
  * Relation: n Drug Packaging (original packagings)
* Manufacturer (Hersteller)
  * Name
  * Address (structured, optional, see Modeling Conventions)
  * Relation: n Drug
* Drug (Medikament)
  * Name
  * Submission Receipt (Abgabebeleg) (bool)
  * Narcotic (Betäubungsmittel, BTM) (bool)
  * Vaccine (Impfstoff) (bool)
  * Refrigerate (Kühlware) (bool)
  * VAT Percent (selector)
  * Approval Number (Zulassungsnummer) (optional)
  * Redesignation (Umwidmung) (bool)
  * Relation: 1 Manufacturer
  * Relation: n Drug Packaging (exactly one of them has Kind = original)
  * Decision: the original packaging is modeled as a Drug Packaging (Kind = original), so invoice positions uniformly reference packagings and the subset price formula stays within one entity
* Drug Packaging (Verpackungseinheit, Teilmenge)
  * Kind (enum: original | subset) (see Modeling Conventions)
  * Unit
  * Quantity
  * List Price Net (Listenpreis Netto) (entered for original packaging; for subsets there is a formula, see AMPreisV)
  * Human Preparation (Humanpräparat, bool) — a medicine approved for humans, dispensed for use in
    an animal. Selects AMPreisV § 3 Abs. 1 Satz 2 (3 % + 8,10 €) instead of the veterinary bands of
    Abs. 3/4; see prices.md.
  * Sales Price Gross (Verkaufspreis Brutto) (formula for original packaging; for subsets: surcharge)
  * Relation: 1 Drug (at most one original packaging per drug: partial unique index on `(drug_id) WHERE kind = 'original'`)
  * Relation: 1 Supplier (original packaging only, CHECK-enforced; subsets have no supplier)
  * Relation: n Drug Stock Lot (original packaging only)
* Drug Stock Lot (Wareneingang) - one delivery of one batch
  * Date of arrival
  * Packages received (number of original packagings)
  * Initial Quantity (base units; snapshot at arrival: packages received × packaging quantity at that time)
  * Batch Number (Chargennummer) (optional)
  * Expiration Date (Verfallsdatum) (optional)
  * Relation: 1 Drug Packaging (Kind = original, enforced via composite FK on `(packaging_id, kind)` + CHECK)
  * Relation: n Drug Stock Movement
* Drug Stock Movement (Bestandsbewegung)
  * Kind (enum: dispense | correction) (see Modeling Conventions)
  * Quantity (base units, signed; CHECK: dispense < 0, correction may be positive or negative)
  * Date
  * in case of dispense:
    * Relation: 1 Treatment Item (one Treatment Item line maps to n movements when it spans several lots)
  * in case of correction (expired drugs, broken packaging, inventory differences, ...):
    * Reason (optional)
    * Relation: 0..1 Drug Stock Movement (reverses — the movement this correction compensates, e.g. on invoice cancellation)
  * Relation: 1 Drug Stock Lot

#### Stock Bookkeeping (Bestandsführung)
* Current stock is always derived, never stored: per lot = Initial Quantity + Σ movement quantities; per drug = Σ over the lots of its original packaging
* Traceability: batch → lot → movements → treatment item → treatment/invoice/customer, so "where did this batch go" is one chronological query over movements
* On dispense the system suggests the lot by FEFO (earliest expiry first); the user confirms or overrides so the recorded batch matches the physically used package — this writes draft dispense movements
* Dispense movements are drafts while the treatment's invoice is not yet Accepted (freely recalculated on edit); they are frozen when the invoice is Accepted
* Movements are append-only once the invoice is Accepted ("Storno statt Löschen"): cancelling an invoice produces compensating corrections, each linked to the dispense it reverses
* Prices are unaffected by stock reversals: movements carry no money, all prices are pinned on Treatment Item at line entry

### Services (Leistungen)
There are two types of services: self defined (like lab costs) and schedule of fees (GOT, Gebührenordnung für Tierärzte). Decision: single entity with an explicit type discriminator (see Modeling Conventions); a CHECK constraint ties GOT Number and Factor presence to the type: `type <> 'got' OR (got_number IS NOT NULL AND factor IS NOT NULL)`. A self-defined service may name the GOT position it is charged analogously to (§ 8 GOT); that number is optional and is what marks it as such — no second flag — and the invoice prints it as `GOT-Nr. <number> (§8)`.
Fields:
* Type (enum: GOT | self-defined)
* Name
* GOT Number (mandatory for a GOT service; optional on a self-defined one, naming the position it follows under § 8 GOT)
* Factor (default 100%, mandatory for GOT, optional for non-GOT)
* VAT Percent (selector)
* Gross Price
* Travel Expenses (bool)
* Hidden (bool)

### Treatment Templates (Behandlungsgruppen)
A template set of Drugs and services for Treatment Items / Invoices
* Treatment Template
  * Name
  * Relation: n Treatment Template Item
* Treatment Template Item
  * Position (for ordering within the group)
  * Relation: 1 Service XOR 1 Drug Packaging incl. unit, amount (XOR: see Modeling Conventions)
  * Relation: 1 Treatment Template

### Master Data (Stammdaten)
* Customer
  * Name (structured, see Modeling Conventions: salutation, optional first name, last name)
  * Company (optional) — some customers are businesses. It prints as the *first* line of the
    invoice address, above the name. First and last name stay mandatory, so there is never a
    company-only recipient and the salutation is unaffected.
  * Second Name (optional, same structure — both persons are addressed on the invoice, e.g. non-married couples)
  * Home Address (structured, see Modeling Conventions; mandatory — the place to drive to)
  * Invoice Address (structured, optional) with its own recipient name (optional company, salutation, optional first name, last name); when set, invoices use it — otherwise invoices use the customer name(s) and home address. The company belongs to the recipient, so the invoice address has its own and never borrows the home one.
  * E-Mail Address (multiple, with type)
  * Phone Number (single, optional; validated, see Modeling Conventions)
  * Warning Remark (Warnhinweis) (e.g. dangerous animal etc., prominently displayed) (optional)
  * Relation: n Patients
* Patient (Tier)
  * Name
  * Sex
  * Species
  * Race (optional)
  * Colour (optional)
  * Weight in kg (optional, at most one fractional digit — the form rounds as you type, so 4,25
    is stored as 4,3). A single **current** value, deliberately not a per-visit history: each
    weighing replaces the last. If the weight at the time of a treatment is ever needed, it moves
    onto Treatment and the patient shows the most recent one.
  * Date of Birth (optional)
  * Photo (optional, references an Attachment)
  * Date of Death (optional)
  * Chip Number (optional)
  * EU Vaccination Passport Number (EU-Pass-Nr.) (optional)
  * Warning Remark (Warnhinweis) (optional)
  * Neutered (bool)
  * Insured (bool)
  * Relation: n Attachment (customer provided files, with Reference Date and Note)
  * Relation: n Treatment

### Appointments (Termine)
* Appointment
  * Date + Time (minute resolution)
  * Note (optional)
  * Relation: n Treatment
* Treatment
  * Relation: 1 Appointment
  * Relation: n Patient (all patients of a treatment must belong to the same customer — the invoice's customer derives from it; enforced when adding patients)
  * Treatment Reason (Vorstellungsgrund) (optional)
  * Finding (Diagnosis) (optional)
  * Relation: n Attachment (treatment files)
  * Relation: n Treatment Item (with explicit ordering)
  * Relation: n Invoice (at most one not Cancelled; cancelled invoices are retained for bookkeeping)
* Treatment Item (Behandlungsposition) - either a Drug Packaging or a Service (XOR: see Modeling Conventions). This is the basis for invoice generation so all values must be copied so that later on changing a price, name, etc doesn't change existing invoices. Values are copied when the line is added to the treatment (pinned to the time of treatment), not at invoice generation
  * in case of Drug Packaging:
    * Quantity
    * Unit
    * allow to override Name
    * Relation: n Drug Stock Movement (dispense; one per lot used)
    * Relation: 1 Patient (mandatory — which patient received the drug, dispensing documentation; CHECK-enforced per Modeling Conventions)
  * in case of Service:
    * Quantity
    * Factor
    * GOT Number (copied, for GOT services)
    * allow to override Name
    * Relation: 0..1 Patient (optional)
  * Position (for ordering)
  * Price (copied from the current sales price; allow to override)
  * VAT Percent (copied)

### Invoice (Rechnung)
Status is the lifecycle; the timestamps are the audit record (e.g. Timestamp Cancelled records when cancellation happened). Listings exclude Cancelled invoices unless stated otherwise.
Fields:
* PDF File (references an Attachment)
* Invoice Number (string!)
* Invoice Date
* Email Recipients (optional)
* Timestamp Sent by Email (optional)
* Timestamp Sent by Post (optional)
* Timestamp Submitted to Bookkeeping (optional)
* Timestamp Cancelled (optional)
* Includes Finding (bool)
* Note (Anmerkung) (optional)
* Status (enum: Created, Accepted, Sent, Submitted to Bookkeeping, Cancelled) — Sent is a precondition of Submitted
* Relation: 1 Treatment

### Invoice Number Sequence (Rechnungsnummernkreis)
DB state for invoice number allocation (the pattern itself lives in global settings):
* Scope (e.g. year, depending on the pattern)
* Counter (monotonic, never decremented)
* Numbers are never reused: Created invoices keep their number on update; numbers of Accepted invoices are burned on cancellation

### Attachment (Datei)
Content-addressed file storage on the filesystem (see tech decisions doc), metadata in the DB:
* SHA-256
* MIME Type
* Size (bytes)
* Original Name
* Kind (enum: patient file | treatment file | referenced) (see Modeling Conventions)
* in case of patient file (customer provided):
  * Reference Date (not upload date or creation date) (optional)
  * Note (optional)
  * Relation: 1 Patient
* in case of treatment file:
  * Relation: 1 Treatment
* referenced: no owner relation — the owning entity points at the attachment (Patient Photo, Invoice PDF, practice logo)

### Global Settings (Einstellungen)
Single row, editable on the settings page (see global settings doc for the DB-vs-config split):
* Practice Name
* Practice Address
* IBAN
* UStID
* Logo (optional, references an Attachment)
* Global CC/BCC E-Mail Addresses