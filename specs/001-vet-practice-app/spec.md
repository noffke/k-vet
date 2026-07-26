# Feature Specification: Vet Practice Management Application

**Feature Branch**: `001-vet-practice-app`

**Created**: 2026-07-26

**Status**: Draft

**Input**: User description: "Build an application based on the description in requirements/logic.md and requirements/datamodel.md together with requirements/global settings.md and requirements/prices.md"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Record a Treatment and Issue an Invoice (Priority: P1)

The vet sees one or more animals during an appointment, records what was done and which drugs
were dispensed, and turns that record into an invoice: inspect the generated PDF, accept it,
and email it to the customer. This is the core daily loop of the practice.

**Why this priority**: Treatment documentation and billing is the reason the application
exists; every other capability feeds this loop.

**Independent Test**: With a customer, a patient, one service, and one stocked drug present,
record a treatment with two line items, create the invoice, accept it, and verify the PDF and
the email hand-off.

**Acceptance Scenarios**:

1. **Given** an appointment exists, **When** the vet adds a treatment with a treatment reason
   and adds line items (drugs and services) via the unified search, **Then** each line is
   stored with name, price, VAT, and (for services) factor copied at the moment the line is
   added, so later catalog changes never alter this treatment.
2. **Given** a treatment with exactly one patient, **When** a line item is added, **Then** that
   patient is preselected on the line; drug lines always require a patient, service lines may
   leave it empty.
3. **Given** a drug line is added, **When** the system proposes stock, **Then** the lot with the
   earliest expiration date that still has stock is suggested and the vet can confirm or pick a
   different lot; the resulting stock deductions stay revisable drafts while the treatment is
   being edited.
4. **Given** a treatment template exists, **When** the vet applies it to a treatment, **Then**
   its items are appended in template order with current prices and names copied.
5. **Given** a finished treatment, **When** the vet creates the invoice with the
   "Includes Finding" option and an optional note, **Then** an invoice in state Created is
   produced and its PDF is presented for inspection.
6. **Given** a Created invoice, **When** the vet accepts it, **Then** the vet can select one or
   more of the customer's email addresses and send the PDF; the stock deductions of the
   treatment are frozen at that moment.
7. **Given** an Accepted invoice whose treatment must be changed (e.g. the finding changed),
   **When** the vet updates the invoice, **Then** the old invoice is cancelled automatically,
   its number is never reused, and a new invoice is created; a Created (not yet accepted)
   invoice keeps its number on update.
8. **Given** any input field on the treatment screen, **When** the vet types and navigates away
   or reloads, **Then** the entered data is present without any explicit save action.

---

### User Story 2 - Manage Customers and Patients (Priority: P2)

The vet maintains the practice's master data: customers with their addresses and email
addresses, and their animals with identifying details, photo, and warnings.

**Why this priority**: Treatments and invoices hang off customers and patients; the practice
also uses this as its day-to-day animal index.

**Independent Test**: Create a customer with two email addresses and a warning remark, add a
patient with photo and species, archive and un-archive records, and verify warning display and
search behavior.

**Acceptance Scenarios**:

1. **Given** a customer with a warning remark (e.g. dangerous animal), **When** the vet opens
   the customer, **Then** the warning is prominently displayed; in list views a warning
   indicator with the text as tooltip is shown. The same applies to patient warning remarks.
2. **Given** a new patient is created, **When** the species is chosen, **Then** Cat and Dog are
   offered as defaults and any free-text species can be entered instead.
3. **Given** a patient with a photo, **When** the patient record is opened, **Then** the photo
   is shown in a circle at the top.
4. **Given** a patient's date of death is set, **When** the record is saved, **Then** the
   patient is archived automatically.
5. **Given** archived customers or patients, **When** the vet searches or uses select boxes,
   **Then** archived records are hidden by default with an option to show them; master data
   referenced by invoiced treatments can never be permanently deleted.
6. **Given** a patient, **When** the vet uploads customer-provided files, **Then** each file
   can carry a reference date (distinct from the upload date) and a note.
7. **Given** the customer form, **When** an email address or phone number with invalid syntax
   is entered, **Then** the field shows a validation error, the invalid value is not saved, and
   auto-save retains the last valid state; a valid phone number is redisplayed in the familiar
   national format regardless of how it was typed.

---

### User Story 3 - Run the Pharmacy: Drugs, Prices, and Stock (Priority: P2)

The vet maintains drugs and their packagings, lets the system derive sales prices from list
prices, records incoming deliveries as lots, corrects stock, and can trace any batch to the
patients that received it.

**Why this priority**: Drug dispensing is both a revenue stream and a regulatory traceability
duty; treatments cannot bill drugs without it.

**Independent Test**: Create a drug with an original packaging and a subset packaging, verify
the computed sales prices, record a stock intake, dispense via a treatment, apply a correction,
and read the lot's movement history.

**Acceptance Scenarios**:

1. **Given** a drug packaging, **When** the vet selects the VAT rate and enters the list price,
   **Then** the sales gross price is computed according to the statutory veterinary price rules
   (including the surcharge for subset packagings) and can be manually overridden.
2. **Given** a delivery arrives, **When** the vet records a stock intake for an original
   packaging with arrival date (default today), number of packages, and optional batch number
   and expiration date, **Then** a lot is created whose initial quantity is the number of
   packages times the packaging quantity at that time.
3. **Given** a lot with remaining stock, **When** the vet records a correction, **Then** the
   quantity defaults to the lot's remaining stock and the reason can be picked from previously
   used reasons (alphabetical) or entered as new text.
4. **Given** a batch number, **When** the vet opens the lot detail, **Then** all movements are
   listed chronologically, each linking to the treatment/invoice/customer that received a
   dispense or showing the correction reason.
5. **Given** an invoice is cancelled, **When** the cancellation completes, **Then** every frozen
   stock deduction of that treatment is compensated by a linked counter-movement; the history
   is never rewritten.
6. **Given** any drug or lot, **When** stock is displayed, **Then** it is always derived from
   the movement history, never stored as an editable number.

---

### User Story 4 - Maintain the Service Catalog and Treatment Templates (Priority: P3)

The vet manages billable services — the imported official fee schedule (GOT) and self-defined
services — and groups recurring combinations of services and drugs into reusable templates.

**Why this priority**: The GOT catalog and templates make treatment entry fast and correct, but
a minimal system could operate with hand-maintained services.

**Independent Test**: Verify the imported GOT catalog is present with surgical positions
hidden, create a self-defined service, create a template, and reorder its items.

**Acceptance Scenarios**:

1. **Given** the application is set up, **When** the vet searches services, **Then** the
   official GOT 2022 positions are available, with surgical positions hidden; hidden GOT
   services are excluded from pickers and lists by default.
2. **Given** a GOT service, **When** it is maintained, **Then** it carries its GOT number and a
   factor (default 100%); self-defined services have no GOT number and an optional factor.
3. **Given** a service flagged as travel expense, **When** it is added to a treatment, **Then**
   the vet enters the kilometers driven and the price is computed from the official travel-fee
   rule (rate per double kilometer with the legal minimum) and appears as a normal invoice
   line.
4. **Given** a treatment template, **When** the vet reorders its items, **Then** items can move
   one step up or down and jump to top or bottom.

---

### User Story 5 - Invoice Overview and Bookkeeping Hand-off (Priority: P3)

The vet reviews all invoices, downloads PDFs, cancels wrong invoices, and periodically hands
accepted invoices to the bookkeeper in bulk.

**Why this priority**: The monthly bookkeeping hand-off is a recurring chore that the invoice
list turns from manual collation into two clicks.

**Independent Test**: With several accepted invoices, verify list ordering, bulk download of
all not-yet-submitted PDFs, and bulk marking as submitted.

**Acceptance Scenarios**:

1. **Given** the invoices page, **When** it loads, **Then** Accepted invoices not yet submitted
   to bookkeeping are listed on top, and cancelled invoices are excluded from listings by
   default.
2. **Given** invoices not yet submitted, **When** the vet uses bulk download, **Then** all their
   PDFs are downloaded together, and a separate bulk action marks them all as submitted with a
   timestamp.
3. **Given** any invoice, **When** the vet opens it, **Then** the PDF can be viewed and
   downloaded, and the invoice can be cancelled.

---

### User Story 6 - Dashboard and Practice Settings (Priority: P4)

The vet lands on a dashboard showing what needs attention and maintains practice-wide settings
(practice name and address, bank account, tax ID, logo, bookkeeper email in CC/BCC) on a
settings page.

**Why this priority**: Convenience and configuration; the practice can operate without it, but
it surfaces expiring stock and pending bookkeeping work.

**Independent Test**: Verify the dashboard counts and links, and that settings changes are
reflected on the next generated invoice.

**Acceptance Scenarios**:

1. **Given** invoices not yet submitted to bookkeeping exist, **When** the dashboard is opened,
   **Then** their count is shown and links directly to the corresponding invoice list.
2. **Given** lots with stock and expiration dates, **When** the dashboard is opened, **Then**
   the 5 lots expiring soonest that still have stock are listed, each navigable to the lot.
3. **Given** the settings page, **When** the vet edits practice name, address, IBAN, tax ID
   (UStID), uploads a logo, or sets global CC/BCC email addresses, **Then** subsequent invoices
   and emails use the new values.

---

### Edge Cases

- A dispense larger than the suggested lot's remaining stock is split across multiple lots
  (earliest expiration first); one treatment line may therefore map to several stock movements.
- If total stock across all lots is insufficient, the vet is warned but may record the dispense
  anyway (the physical shelf is the truth; a later correction reconciles the difference); the
  shortfall is booked against the FEFO-suggested or user-chosen lot, whose derived stock may
  go negative.
- Editing a treatment whose invoice is already Accepted requires the update flow: the old
  invoice is auto-cancelled (number burned), stock reversals are written, and editing resumes
  with draft stock movements.
- A duplicated appointment or treatment asks whether prices are copied verbatim or refreshed
  from the current catalog; the duplicate's date defaults to today.
- Archived drugs, services, customers, or patients remain fully visible inside historical
  treatments and invoices even though pickers hide them.
- The travel-expense computation applies the legal minimum when the entered distance is short.
- Emailing an invoice that fails to send leaves the invoice Accepted; the send can be retried
  and the sent-timestamp is only set on success.
- Invoice numbers continue monotonically within their scope (e.g. per year); a burned number
  (cancelled Accepted invoice) is never reissued.
- Two browser tabs editing the same record: last write wins; auto-save never silently drops
  a change without it having been visible in the UI state.
- Auto-save and invalid input: fields that fail validation (email, phone) are not persisted;
  the field keeps the invalid text visible with an error until corrected, while the stored
  record keeps the last valid value.
- Invoice addressing: without an invoice address the invoice shows the customer name(s) —
  second name as a second line — at the home address; with an invoice address set, the PDF
  shows only the invoice recipient's own name at that address.
- Opening a "new record" form and navigating away without typing leaves no permanent record;
  a partially filled record reappears as "incomplete" (with what's missing) and is resumable.
  Completion happens implicitly with the input that fills the last mandatory field — no
  confirmation step.

## Requirements *(mandatory)*

### Functional Requirements

#### Access & Cross-Cutting UX

- **FR-001**: The system MUST support exactly one user account with login and session; the
  credentials are supplied by the operator's configuration, not managed in the UI.
- **FR-002**: The system MUST auto-save all user input continuously (as in collaborative
  document editors) with no explicit save action anywhere; entered data survives navigation
  and reload. New records are persisted from the first input; records still missing mandatory
  information are marked incomplete, excluded from selection and billing, and can be resumed
  later. A record becomes complete automatically the moment its mandatory fields are filled —
  there is no save or confirm action; clearing a mandatory field on a complete record is
  rejected like invalid input. Abandoned records that never received any input are discarded
  automatically.
- **FR-003**: Every screen MUST be usable on laptop/desktop screens and on a phone-sized screen.
- **FR-004**: The UI MUST be available in German (default) and English.
- **FR-005**: Wherever drugs or services are selected, the system MUST offer a unified picker
  with full-text search over the name, results ranked by how often the entry was used in past
  treatments (usage ranking refreshed at least nightly), and a visual distinction between
  drugs and services.

#### Customers & Patients (Master Data)

- **FR-006**: The system MUST manage customers with a structured name — salutation
  (Frau/Herr/Familie), optional first name, last name — plus an optional second full name of
  the same structure (households where both persons are addressed on the invoice, e.g.
  non-married couples); a mandatory structured home address and an optional invoice address
  with its own recipient name (salutation, optional first name, last name) — each address
  consisting of an optional addon line (e.g. "c/o Fr. Müller"), street including house number,
  ZIP, and city — multiple email addresses with type, an optional phone number, and an
  optional warning remark.
- **FR-007**: Warning remarks on customers and patients MUST be shown as a warning banner at
  the top of the record when it is opened, and indicated in list views via a warning icon with
  the text as tooltip.
- **FR-008**: The system MUST manage patients with name, sex, species (Cat/Dog offered as
  defaults, free text allowed), optional race, colour, date of birth, photo, date of death,
  chip number, EU vaccination passport number, warning remark, and neutered/insured flags; a
  set photo is displayed in a circle on top of the record.
- **FR-009**: Master data (customers, patients, drugs, packagings, services, suppliers,
  manufacturers, treatment templates) MUST be archived instead of deleted; archived records are
  hidden from search and pickers by default with an option to show them; setting a patient's
  date of death archives it automatically; data referenced by invoiced treatments can never be
  permanently removed.
- **FR-010**: The system MUST store customer-provided files per patient, each with an optional
  reference date (distinct from upload time) and note, plus arbitrary files per treatment.

#### Pharmacy & Stock

- **FR-011**: The system MUST manage suppliers and manufacturers (name and an optional
  structured address: addon, street incl. house number, ZIP, city).
- **FR-012**: The system MUST manage drugs with manufacturer, VAT rate, optional approval
  number, and the flags submission receipt (Abgabebeleg), narcotic (BTM), vaccine,
  refrigerated, and redesignation (Umwidmung). In this version these flags are informational
  only — no reminders, documents, or ledgers are generated from them.
- **FR-013**: Each drug MUST have exactly one original packaging and any number of subset
  packagings, each with unit and quantity; only the original packaging has a supplier and
  stock.
- **FR-014**: After VAT selection and list-price entry, the system MUST compute the sales gross
  price per the statutory veterinary drug pricing rules (AMPreisV, veterinarian part),
  including the subset surcharge for subset packagings; the computed price MUST be manually
  overridable.
- **FR-015**: Recording a stock intake MUST create a new lot for an original packaging with
  arrival date (default today), packages received, optional batch number and expiration date;
  the lot's initial quantity is snapshotted as packages received × packaging quantity at that
  time.
- **FR-016**: The system MUST support correction movements per lot; the quantity defaults to
  the lot's remaining stock and the reason is chosen from previously used reasons (sorted
  alphabetically) or entered as new text.
- **FR-017**: A lot detail view MUST show all movements chronologically, each linking to the
  treatment/invoice/customer that received a dispense, or showing the correction reason.
- **FR-018**: Remaining stock MUST always be derived from initial quantity plus the sum of
  movements — never stored or directly editable.
- **FR-019**: When dispensing, the system MUST suggest the lot with the earliest expiration
  date that has stock (FEFO) and let the vet confirm or override; a single treatment line may
  split across several lots.
- **FR-020**: Dispense movements MUST remain drafts (freely recalculated on treatment edits)
  until the treatment's invoice is Accepted, at which point they are frozen; after freezing,
  the movement history is append-only and cancellations write compensating counter-movements
  linked to the movements they reverse.

#### Services & Templates

- **FR-021**: The system MUST manage services of two kinds: official fee schedule (GOT) with
  GOT number and mandatory factor (default 100%), and self-defined with optional factor; both
  carry VAT rate, gross price, a travel-expense flag, and a hidden flag. Hidden services are
  excluded from pickers and lists by default.
- **FR-022**: The full GOT 2022 fee schedule MUST be available as a one-time initial import,
  with surgical positions marked hidden.
- **FR-023**: For services flagged as travel expense, the vet MUST enter the kilometers driven
  and the system computes the price from the official travel-fee rule — rate per double
  kilometer with the legal minimum applied, plus an optional ×1–×3 multiplier for adverse
  travel conditions per the fee schedule — rates configurable by the operator; the result
  appears as a normal line on the invoice.
- **FR-024**: The system MUST manage treatment templates as ordered lists of drug-packaging or
  service items; items can be moved one step up/down and to top/bottom; applying a template to
  a treatment appends its items in order with current prices and names copied.

#### Appointments & Treatments

- **FR-025**: The system MUST manage appointments with date (default today), a time that the
  user must always fill, and an optional note; an appointment can hold multiple treatments.
  Appointments and treatments can be deleted only while no non-cancelled invoice exists for
  them; afterwards they are permanent records.
- **FR-026**: Appointments and treatments MUST be duplicable; the duplicate's date defaults to
  today and the vet chooses whether prices are copied verbatim or refreshed from the current
  drug/service data.
- **FR-027**: A treatment MUST reference its appointment, one or more patients, an optional
  treatment reason and finding, and any number of attached files. All patients of a treatment
  MUST belong to the same customer — that customer is the treatment's (and its invoice's)
  customer; adding a patient of a different customer is rejected.
- **FR-028**: Treatment lines MUST be either a drug packaging (quantity, unit, name override
  allowed, patient mandatory) or a service (quantity, factor, GOT number carried over, name
  override allowed, patient optional); when the treatment has exactly one patient it is
  preselected. Price and VAT are copied onto the line when the line is added and are
  overridable; later catalog changes never affect existing lines.
- **FR-029**: Treatment lines MUST be reorderable: one step up/down, to top/bottom.

#### Invoices

- **FR-030**: Invoices MUST be creatable and updatable only from a treatment. Creation offers
  an "Includes Finding" option and an optional note, produces an invoice in state Created, and
  presents the PDF for inspection. The invoice is addressed to the customer's invoice-address
  recipient when an invoice address is set; otherwise to the customer name(s) — both names on
  separate lines when a second name exists — at the home address. The invoice date is set when
  the invoice is created and is set anew when an update creates the replacement invoice.
  Invoice PDFs and invoice emails are always German (de-DE), regardless of the UI language.
- **FR-031**: Accepting a Created invoice MUST allow selecting one or more of the customer's
  email addresses and emailing the invoice PDF; configured global CC/BCC addresses are
  applied; the sent timestamp is recorded on successful send. A failed or repeated send can be
  retriggered for an Accepted invoice.
- **FR-032**: Updating an invoice MUST keep the invoice number if the invoice is still
  Created; if it is Accepted, the invoice is automatically cancelled first and the new invoice
  gets a fresh number — numbers of Accepted invoices are burned, never reused.
- **FR-033**: Invoice numbers MUST follow an operator-configured pattern with a monotonic
  counter per scope (e.g. per year); the counter never decrements and numbers are never
  reissued.
- **FR-034**: The invoice list MUST show Accepted-but-not-submitted invoices on top, exclude
  Cancelled invoices by default, and support viewing/downloading the PDF, cancelling, and
  marking as submitted to bookkeeping — including bulk download of all not-submitted PDFs and
  bulk marking as submitted.
- **FR-035**: Each lifecycle step (accepted, sent by email, submitted to bookkeeping,
  cancelled) MUST be recorded with a timestamp as the audit trail.

#### Dashboard & Settings

- **FR-036**: The dashboard MUST show the count of invoices not yet submitted to bookkeeping
  with a direct link, and the 5 lots expiring soonest that still have stock, each navigable.
- **FR-037**: A settings page MUST let the vet edit practice name and address, IBAN, UStID,
  uploadable practice logo, and global CC/BCC email addresses; invoices and emails generated
  afterwards use the new values. Infrastructure settings (mail server, invoice number pattern,
  currency, VAT rate choices, credentials, templates) are operator configuration, not editable
  in the UI.

#### Input Validation

- **FR-038**: Every email address input (customer email addresses, invoice email recipients,
  CC/BCC settings) MUST be validated for correct email syntax. Phone number input MUST be
  validated as a real phone number — German conventions by default, international input
  accepted — stored in a normalized form and displayed in the familiar national format.
  Invalid values are rejected with a clear field-level message and are never persisted.

### Key Entities

- **Customer**: A person (or couple) owning animals; structured name (salutation, optional
  first name, last name) with an optional second full name, a mandatory structured home
  address, an optional invoice address with its own recipient name, typed email addresses,
  optional phone number, optional warning remark; owns patients.
- **Patient**: An animal; identity and medical-identity attributes (species, sex, chip number,
  EU passport number, …), optional photo and warning remark, date of death; belongs to a
  customer, has treatments and files.
- **Supplier / Manufacturer**: Address-book entries for where original packagings are bought
  and who produces a drug.
- **Drug**: A medicinal product with regulatory flags, VAT rate, and manufacturer; has exactly
  one original packaging and optional subset packagings.
- **Drug Packaging**: A sellable unit of a drug (original or subset) with unit, quantity, list
  price, and computed-but-overridable sales price.
- **Drug Stock Lot**: One delivery of one batch of an original packaging; arrival date,
  packages received, snapshotted initial quantity, optional batch number and expiration date.
- **Drug Stock Movement**: A signed quantity change on a lot — a dispense (tied to a treatment
  line) or a correction (with reason, optionally reversing another movement); the complete,
  append-only stock history.
- **Service**: A billable act — official fee schedule (GOT, with number and factor) or
  self-defined; VAT rate, gross price, travel-expense and hidden flags.
- **Treatment Template / Template Item**: A named, ordered set of drug-packaging/service items
  for quick treatment entry.
- **Appointment**: A dated visit (minute-resolution time) holding one or more treatments.
- **Treatment**: The medical record of one visit for one or more patients: reason, finding,
  files, ordered billing lines; source of at most one non-cancelled invoice.
- **Treatment Item**: One billing line — drug packaging or service — with all values (name,
  price, VAT, factor, GOT number) pinned at line entry; drug lines attribute a patient and
  drive stock movements.
- **Invoice**: The bill for a treatment; number, date, state (Created, Accepted, Submitted to
  Bookkeeping, Cancelled), lifecycle timestamps, note, "includes finding" flag, generated PDF,
  email recipients.
- **Invoice Number Sequence**: The per-scope monotonic counter backing invoice number
  allocation.
- **Attachment**: A stored file (photo, lab report, invoice PDF, logo) with content identity,
  type, size, and original name; owned by a patient or treatment, or referenced by other
  records.
- **Global Settings**: The single practice-wide record of name, address, IBAN, UStID, logo,
  and global CC/BCC addresses.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A routine treatment (two drugs, two services) is recorded and its invoice
  accepted and emailed in under 3 minutes.
- **SC-002**: Zero data loss from missing save actions: any text entered and visible in the UI
  is still present after navigating away and back or reloading, in 100% of cases.
- **SC-003**: Drug/service search shows ranked results within 1 second, with the practice's
  most-used entries first.
- **SC-004**: For any batch number in stock history, the full list of receiving
  customers/patients is readable from one view, reachable in under 30 seconds.
- **SC-005**: 100% of computed drug sales prices and travel-expense lines match the statutory
  worked examples to the cent.
- **SC-006**: Invoice numbers are unique and gap-explainable: every number is issued at most
  once, and every burned number corresponds to exactly one cancelled invoice.
- **SC-007**: Derived stock always reconciles: for every lot, initial quantity plus all
  movements equals displayed remaining stock, in 100% of cases including after invoice
  cancellations.
- **SC-008**: The monthly bookkeeping hand-off (download all pending PDFs, mark all submitted)
  completes in under 1 minute.
- **SC-009**: All screens pass a usability check on both a desktop viewport and a
  phone-sized viewport (no horizontal scrolling, all actions reachable).
- **SC-010**: The complete UI is available in German and English with German as default; no
  user-facing text appears only in one language.

## Assumptions

- Decisions taken during specification (also recorded in the requirements documents):
  - Narcotic (BTM) and Submission Receipt (Abgabebeleg) flags are informational only in this
    version — no reminders, receipts, or ledgers; the practice's paper processes continue.
  - Travel expenses are computed from a kilometer input using the official travel-fee rule
    (rate per double kilometer, legal minimum), with rates operator-configurable.
  - Drug sales prices are computed per AMPreisV only; a configurable "list price + VAT" mode
    is deferred.
- Single-user system: one vet account, no roles, no concurrent-user handling beyond
  last-write-wins between the user's own sessions.
- Appointment *scheduling* stays in the practice's external calendar; appointments here are
  documentation records that group treatments, with no calendar synchronization.
- The GOT 2022 fee schedule import is a one-time provisioning step; keeping it current with
  future GOT amendments is manual maintenance.
- A single currency is used practice-wide; VAT rate choices offered for new drugs/services are
  the German normal (19%) and reduced (7%) rates, while every stored line keeps its own rate.
- Insufficient stock does not block dispensing: the vet is warned and stock may go negative
  until reconciled by a correction.
- Everything listed in `requirements/future ideas.md` is out of scope for this version, as are
  multi-user support, customer self-service, and any integration beyond outbound email.
