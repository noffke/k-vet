# Business Logic, Functionality and UI/UX considerations
## Selecting Drug Packagings or Services
There are various places in the UI where Drug Packagings or Services have to be selected. This needs a unified
select box component. Maybe we should mark drugs and services with a different icon for visual separation? The select
has to be a full text search on drug name or service. Search results should be weighted by how often the drug
or service was used in a treatment. It's sufficient to update weights once at night.

## Pharmacy (Apotheke)
For drugs and packagings, we need normal CRUD functionality. After selecting VAT and entering the list price, price calculation logic should be used to calculate the sales gross price.

Recording a stock intake (Wareneingang) creates a new lot for an original packaging: date of arrival (default today), packages received, batch number and expiration date (both optional). The initial quantity is snapshotted from the packaging definition (packages received × packaging quantity).

For a drug stock lot, we need the option to do a correction movement. The default quantity should be the remainder of the stock of the lot. The reason should be an editable
select that gives previous reasons (alphabetically sorted) with the option to clear and add a new reason.

Decision (revised): a lot's remaining stock must never go below zero, and no movement may take it there — neither a correction nor a dispense. Earlier this held only for what was
counted: a dispense that exceeded the books was recorded in full and the lot's derived stock went negative, on the grounds that the physical shelf is the truth. In practice a negative
remainder is not a record of reality, it is a record that the books were already wrong, and it propagates silently — FEFO then picks lots that hold nothing. So a dispense that a lot
cannot cover is refused and the vet is pointed at the remedy: the stocktake correction that already exists, entered against what is physically on the shelf. The message names the
remedy rather than the shortfall — field errors carry no interpolated values, and the numbers are on the lot page she is being sent to anyway; the shortfall is logged. The cost is
an interruption mid-consultation, accepted deliberately: it lands at the moment the discrepancy is discovered, which is when it can still be counted.

For batch traceability, a lot detail view must show all movements of the lot chronologically, each linking to the corresponding treatment/invoice/customer (dispenses) or showing the reason (corrections).

Decision: in v1 the Narcotic (BTM) and Submission Receipt (Abgabebeleg) flags are informational only — no reminders, no receipt generation, no BTM ledger; the existing paper processes continue. Revisit after v1.

## Services (Leistungen)
For both self defined and GOT services we need normal CRUD functionality. Hidden GOT services should be hidden by default.
For the GOT services, we need a one-time import derived from GOT_2022.pdf. This should be a set of SQL inserts
that should be added as a migration. If possible, mark all GOT positions for surgical services as hidden (no surgery offered for the time being).

Decision: for services flagged Travel Expenses, the vet enters the kilometers driven; the price is computed per the GOT Wegegeld rule (rate per double kilometer with the legal minimum; rates live in the config file) and appears as a normal invoice line.

## Treatment Templates (Behandlungsgruppen)
For treatment templates, we need normal CRUD functionality. Additionally, functionality for changing the position ordering
of the items within a group is needed: move one step up or down, move to top or bottom. 

## Master Data (Stammdaten)
For master data, we need normal CRUD functionality. Master data is archived instead of deleted (soft delete): archived customers and patients are hidden from search and select boxes by default (with the option to show them); patients with a date of death are archived automatically. Data referenced by invoiced treatments is never hard-deleted.

If a warning remark has been set for a customer, it must be prominently displayed when "opening" the customer data.
In a table view, maybe add a warning icon with a tooltip containing the text. Same for warning remarks on patients.

For patients, show the patient photo (if set) in a circle on top. Species should be a select box with Cat and Dog as default, but with the option to clear and enter freetext.

## Appointments (Termine)
For appointments, we need normal CRUD functionality. When creating a new appointment, the date should be set to today by default, and the time must always be filled by the user.
For appointments and treatments, we also need the option to duplicate an existing one. The duplicated appointment should then have the date set to today by default.
Additionally, when duplicating an appointment or treatment, there must be the option to select if prices
should be copied verbatim or updated from drugs and services data.

For treatment items, functionality for changing the position ordering
is needed: move one step up or down, move to top or bottom.

On the Treatment page, a treatment template can be applied: its items are appended (in item order) as treatment items with the current prices and names copied.

Each treatment item line must be attributed to a patient: mandatory for drug lines, optional for service lines. When the treatment has exactly one patient, it is preselected.

On an individual treatment, we need the option to create or update the invoice. When
creating the invoice, we need the option to select a checkbox for Includes Finding. An optional note
can also already be entered. The invoice is then in Created state. The generated PDF should then
be presented to the user for inspection. If ok, we need the option to Accept the invoice. This should
also give the option to select one or more email addresses from master data and then email the invoice PDF to the
customer. Dispense movements remain drafts while the treatment is edited and are frozen (booked) when the invoice is Accepted. There should also
be the possibility to update an invoice (e.g. finding changed, etc.). If the invoice was already Accepted, it
has to be automatically canceled first. Created invoices can keep their invoice number on update, Accepted invoice
numbers are "burned".

The lifecycle is `Created → Accepted → Sent → Submitted`, or `Cancelled` from any of them. An
invoice must have reached the customer before it can be handed to bookkeeping — Sent is a
precondition of Submitted, which is why one status column suffices.

An invoice reaches the customer by one of two routes, and both are recorded with their own
timestamp:

* by email — the timestamp is written only after the mail server accepted the message. Accepting
  the invoice sends it as a side effect; if that fails, the invoice stays Accepted and can be
  sent again from the treatment page, to addresses chosen at that moment (the last send may have
  gone to the wrong one, or to none at all).
* by post — nothing else can observe a letter going into a postbox, so the vet marks it by hand.
  Only an Accepted invoice can be marked as posted.

## Invoice (Rechnung)
Invoices can only be created or updated from a treatment. On the invoices page, it should be possible to view and
download an invoice PDF and cancel an invoice. Additionally, it should be possible to mark invoices
as submitted to bookkeeping, and to mark an Accepted one as sent by post. Sent but not Submitted
to Bookkeeping invoices should be listed on top of the page.
There should be the option to bulk-download all PDFs not submitted for bookkeeping, and bulk-mark them as submitted.

## Dashboard
On the dashboard, we should give the number of invoices not yet submitted to bookkeeping and allow to directly jump to the corresponding page.

There are two ways the practice loses money, and the dashboard tracks both — as three lists,
because each has its own remedy. Each shows the oldest few cases with the customer, the animals,
the date and the amount, links each row to the treatment it is settled on, and says how many
more there are:

* **not billed** — treatments that carry positions and have no live invoice. Only visits before
  today count: a treatment being written up during the visit has positions and no invoice by
  definition, and a dashboard that shouts about the work in hand teaches the vet to ignore it.
* **not released** — invoices in Created.
* **not sent** — invoices in Accepted, including the ones whose email failed on the way out.

The same two cases are marked on the records themselves: appointments and treatments carrying
unbilled work, and released invoices that never went anywhere.

Also, the top 5 drug lots that will expire next (but still have stock) should be listed and be allowed to directly navigate to. 

Decision: no special marking for used-up lots; remaining stock stays derived (a SUM per lot is trivial at this scale). A remaining-quantity view plus a partial index on expiration date serves this widget.