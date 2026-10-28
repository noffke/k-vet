# Business Logic, Functionality and UI/UX considerations
## Selecting Drug Packagings or Services
There are various places in the UI where Drug Packagings or Services have to be selected. This needs a unified
select box component. Maybe we should mark drugs and services with a different icon for visual separation? The select
has to be a full text search on drug name or service. Search results should be weighted by how often the drug
or service was used in a treatment. It's sufficient to update weights once at night.

## Pharmacy (Apotheke)
For drugs and packagings, we need normal CRUD functionality. After selecting VAT and entering the list price, price calculation logic should be used to calculate the sales gross price.

Recording a stock intake (Wareneingang) creates a new lot for an original packaging: date of arrival (default today), packages received, batch number and expiration date (both optional). The initial quantity is snapshotted from the packaging definition (packages received × packaging quantity). The expiration date can be typed as month and year (`03.2027`, as packages print it), which means that month's last day; a full date is taken as typed, since a lot can expire on another day.

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

Decision: a new Wegegeld line fills in its kilometres (one way), and they stay the vet's to change:
* first the kilometres of the customer's most recent Wegegeld line on another visit, so a correction she made carries forward;
* else, with a Google Maps key configured, Google's shortest driving route (not the most fuel-efficient: the vet's choice) from the practice address to the customer's home address, without live traffic so the same addresses give the same distance, in whole kilometres (a half to the even one: 12.5 km is 12, 13.5 km is 14); kilometres typed or taken over from a previous visit stay as they are;
* once the customer's home address or the practice address changes, the customer is marked and the next new line asks Google again; lines and invoices already written keep their kilometres. The mark clears when new kilometres come in, from Google or typed;
* a Behandlungsgruppe with a Wegegeld line fills it in the same way;
* the toast says where the kilometres came from (the visit's date, or Google), or why there are none — a line without them keeps an empty field;
* an icon beside the field asks Google again for that line (only with a key).

No invoice is created, updated or released while a Wegegeld line has no kilometres: it would bill the bare catalogue price. Zero counts as entered.

## Treatment Templates (Behandlungsgruppen)
For treatment templates, we need normal CRUD functionality. Additionally, functionality for changing the position ordering
of the items within a group is needed: move one step up or down, move to top or bottom. 

## Master Data (Stammdaten)
For master data, we need normal CRUD functionality. Master data is archived instead of deleted (soft delete): archived customers and patients are hidden from search and select boxes by default (with the option to show them); patients with a date of death are archived automatically. Data referenced by invoiced treatments is never hard-deleted.

If a warning remark has been set for a customer, it must be prominently displayed when "opening" the customer data.

A customer's home and invoice addresses are checked against Google (Address Validation) once the vet has typed one and moved on — with a key configured, and never merely by opening the customer. It is a hint, never a block, but a strict one: "Adresse gefunden" only when Google confirms every part down to the house; an address Google writes differently in any part (a spelling, a postcode) offered as Google has it, the differing part in bold, to take over; a part Google cannot confirm named ("Nicht sicher bestätigt: PLZ — bitte prüfen"); or "nicht gefunden — bitte prüfen". Nothing of the answer is stored.

The customer page and the appointment link the customer's home address to Google Maps, opening the drive there (on the phone, the Maps app with the route from where the vet is). The links need no key.
The customer page lists the customer's animals and all of the customer's appointments, newest first, each opening the appointment.

Every list loads page by page: the first fifty rows, then "Mehr laden" for the next fifty while there are more — a list that ends without the button is complete.
In a table view, maybe add a warning icon with a tooltip containing the text. Same for warning remarks on patients.

For patients, show the patient photo (if set) in a circle on top. Species should be a select box with Cat and Dog as default, but with the option to clear and enter freetext.

## Appointments (Termine)
For appointments, we need normal CRUD functionality. When creating a new appointment, the date should be set to today by default, and the time must always be filled by the user.
The appointments page has two tabs: "Anstehend" — from today on, earliest first, under a heading per day ("Heute", "Morgen", then the date), appointments without a date on top — and "Vergangen", newest first. The search works within the open tab.
Calendar on the phone (decided 2026-10-28): k-vet serves its appointments as a CalDAV calendar at `/dav`, synced by a sync app on the vet's phone over the practice's VPN, signed in with the k-vet login. No Google Workspace or other third party: customer data does not leave the practice. Each entry shows the customer's name, the animals with their species (never the breed), the home address, the phone number, the note, the drugs to bring and a link to the appointment; it lasts a fixed time from the configuration, since k-vet keeps only a start. Entries made on the phone become appointments without a customer; customers whose name — or an animal's — appears in the title are suggested, never assigned. A moved entry moves its appointment unless the invoice is written; a deleted one deletes an empty appointment and only hides one with something recorded on it. All-day and recurring entries are kept as sent, not turned into appointments.
"Mitbringen": a customer phones for more of a regular medication (painkillers, …), and the vet notes it on the future appointment — a drug packaging from the pharmacy, a quantity, an optional note. Such a reminder dispenses nothing and names no animal. At the visit, "Als Position übernehmen" makes it a line of the treatment for the animal (chosen when there are several), priced and dispensed FEFO like a picked drug, refused like one when the lots cannot cover it; removing that line makes it a reminder again. The **Packliste** shows a day's open reminders, added up per packaging with the drug's stock checked against the day's whole need (a bottle and its subsets draw from the same lots), and appointment by appointment in time order; each item is ticked off ("gepackt") and that is saved at once.
An appointment has exactly one treatment, created with it; appointment and treatment are edited on one screen. A second invoice for the same visit is a second appointment for the same date, time and customer.
Below the appointment's own fields, each animal of the visit is a tab: its warning, its reason and finding, its files and its lines. A second animal is a second tab, not a different layout. Under each animal's lines, every tab also lists the visit's own positions ("pro Termin": house-call fee, Wegegeld) — the same lines in every tab, billed once. They are picked in an animal's tab like any other position and go there by themselves, because their service is marked "pro Termin"; before any animal is on the visit, only they can be added.
An animal can come off the visit until the invoice is released — not the customer's only animal. Asked first, with what goes named (positions, texts, files): removed, everything recorded for it on this visit goes too, draft dispenses back on the shelf. Or, when another animal stays, it moves to an appointment of its own — same customer and time, its own invoice — with its record, lines and files; the visit's own positions stay.
For appointments, we also need the option to duplicate an existing one, which copies its treatment along. The duplicated appointment should then have the date set to today by default.
Additionally, when duplicating an appointment, there must be the option to select if prices
should be copied verbatim or updated from drugs and services data.

For treatment items, functionality for changing the position ordering
is needed: move one step up or down, move to top or bottom.

The price of a GOT position is the fee schedule's and cannot be changed on the line — the factor (Steigerungssatz) is how it is adjusted. A self-defined service, including one charged by analogy (§ 8 GOT), and a drug line may have their price overridden.

On the Treatment page, a treatment template can be applied: its items are appended (in item order) as treatment items with the current prices and names copied.

Each treatment item line is attributed to exactly one patient of the treatment — unless its service is marked "pro Termin", in which case it belongs to the whole visit and to no patient, also when there is just one animal. The service decides, once, in the catalogue; a line cannot be moved between a patient and the visit. When the treatment has exactly one patient, it is preselected. The reason: drug dispensing and treatment are attributed to a single animal, while the house-call fee and Wegegeld are independent of any individual animal — those stand without an animal, last in the position list and on the invoice.

On an individual treatment, we need the option to create or update the invoice. When
creating the invoice, we need the option to select a checkbox for Includes Finding. An optional note
can also already be entered. The invoice is then in Created state. The generated PDF should then
be presented to the user for inspection. If ok, we need the option to Accept the invoice. This should
also give the option to select one or more email addresses from master data and then email the invoice PDF to the
customer. Dispense movements remain drafts while the treatment is edited and are frozen (booked) when the invoice is Accepted. There should also
be the possibility to update an invoice (e.g. finding changed, etc.). If the invoice was already Accepted, it
has to be automatically canceled first. Created invoices can keep their invoice number on update, Accepted invoice
numbers are "burned".

The PDF is made once, with the invoice; the treatment can change after. When what the PDF prints no longer matches —
a line new, changed, removed or moved, an animal's report changed (the finding only when it is printed) — the treatment
says so first on its page ("PDF veraltet", with what changed and "Rechnung aktualisieren"), the changed lines and
reports are marked, and the appointment carries a "PDF veraltet" chip in the lists. A change taken back is no change.

Accepting an invoice with a euthanasia drug on a line ends that animal's record: its date of death becomes the appointment's day and it is archived, as entering a date of death by hand does. A date already entered stays; accepting again after an update changes nothing, and cancelling does not undo it. The accept page says so before the button.

The lifecycle is `Created → Accepted → Sent → Submitted`, or `Cancelled` from any of them. An
invoice must have reached the customer before it can be handed to bookkeeping — Sent is a
precondition of Submitted, which is why one status column suffices.

An invoice reaches the customer by one of two routes, and both are recorded with their own
timestamp:

* by email — the timestamp is written only after the mail server accepted the message. No
  request talks to the mail server: accepting the invoice with addresses ticked, or sending it
  again from the treatment page (to addresses chosen at that moment — the last send may have gone
  to the wrong one, or to none at all), puts the mail in a queue. Every five minutes the queue is
  worked through; a failure is kept with the mail server's own words and tried again on every
  run until six hours after the first failure, then given up — at once when the mail server
  refused for good (a 5xx answer, an address it does not take). One waiting mail per invoice:
  sending again replaces it. Cancelling the invoice, or marking it as posted, drops the waiting
  mail. Until it is delivered the invoice stays Accepted, and the treatment page says where the
  mail is.

  Releasing or sending also says which email template the mail is written with — one of those
  listed in the operator configuration, the first preselected; one whose file is missing is
  offered disabled, naming the file; the delivery records which one it went out with, next to
  its addresses — and when the mail may go out: "Später versenden" (the default) at a chosen
  day and hour, offered as tomorrow at 08:00 local time, or "Gleich versenden" with the next
  run. A scheduled mail shows its time on the treatment page, in the invoices list and in the
  dashboard's "not sent" list, so it does not look forgotten. The send page reopens on what was
  chosen for a mail already waiting — its schedule, or "Gleich" when it is due with the next
  run. Every request carries the time — "Gleich" is now — and a time already past simply means
  the next run.
* by post — nothing else can observe a letter going into a postbox, so the vet marks it by hand.
  Only an Accepted invoice can be marked as posted.

## Preisauskunft (price quote)
A caller asks what a treatment would cost. The Preisauskunft page puts the positions together — drugs, services, Behandlungsgruppen (expanded into their positions) — with quantity, factor and, for Wegegeld, kilometres, and shows the gross total to tell the caller, with net and VAT per rate underneath.

Decisions:
* A quote is saved as it is put together, like every record, and can carry a name and a note. With a customer it is kept and listed on the customer's page ("Preisauskünfte": name, date, gross), from where it opens again. Without one it is open: leaving it asks "Behalten" (attach a customer — an existing one, or a new one made from the caller's name and the typed address), "Verwerfen" or "Abbrechen". Open quotes are listed as "Offene Preisauskünfte" and removed 28 days after their last change.
* It is priced on the server exactly as a treatment's lines are, so the total is what the invoice would show at those factors. The page never computes an amount. While a quote is being changed it is priced at today's catalogue; left alone it keeps what it was quoted at, and where the catalogue has moved since, the line and the total say so ("heute … statt …").
* On a visit of the customer, a kept quote can be taken over into an animal's tab like a Behandlungsgruppe ("Aus Preisauskunft übernehmen"): its positions, quantities, factors and kilometres, at the prices quoted — or at today's, which the dialog offers where they differ. For a GOT position the schedule's current fee binds, and the dialog says so beside a changed one. Per-visit positions go to the visit. It can be undone like a group.
* One total at each position's usual factor; the vet can change quantity, factor and kilometres per line. No "von – bis" range.
* For Wegegeld, the caller is either an existing customer — kilometres as for a new line: their last visit's, unless their address changed since, else Google's shortest route — or a new caller, for whom an address can be typed: it is checked like a customer's address and asked of Google for the route. Asking never clears a customer's "address changed" mark. Kilometres the vet typed are never overwritten; without kilometres the line costs the catalogue price.
* On the phone it sits first under "Öffnen".

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

Above them, every invoice not yet at the customer (Created or Accepted) whose PDF is out of date — all of them, since
each has to be updated before it goes out. Above that, every invoice mail that failed — still being retried or given up — with the mail
server's answer, to be tried again (from the start, with six fresh hours) or removed from the
queue, which leaves the invoice Accepted for the vet to send again or post.

The same two cases are marked on the records themselves: appointments and treatments carrying
unbilled work, and released invoices that never went anywhere.

Also, the top 5 drug lots that will expire next (but still have stock) should be listed and be allowed to directly navigate to. 

Decision: no special marking for used-up lots; remaining stock stays derived (a SUM per lot is trivial at this scale). A remaining-quantity view plus a partial index on expiration date serves this widget.