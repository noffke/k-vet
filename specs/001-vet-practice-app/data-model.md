# Phase 1 Data Model: Vet Practice Management Application

**Date**: 2026-07-26 | **Spec**: [spec.md](spec.md) | **Source**: `requirements/datamodel.md`

Conventions (from `requirements/datamodel.md` and the constitution):

- Every entity has its own sequence-based PK (`BIGINT GENERATED ALWAYS AS IDENTITY`).
- Every table has `created_at`/`updated_at TIMESTAMPTZ NOT NULL` (trigger-maintained `updated_at`).
- Variants use an explicit discriminator enum column; CHECK constraints tie variant-specific
  columns to the declared kind. XOR relations: one table, kind enum, two nullable FKs, CHECKs.
- Variants map to Rust enums, exposed in the API as tagged unions.
- Master data is archived (`archived BOOLEAN NOT NULL DEFAULT false`), never hard-deleted once
  referenced by invoiced treatments.
- Money `NUMERIC(10,2)`, quantities `NUMERIC(10,2)`, factors/VAT `NUMERIC(7,3)` (percent).
- **Address column group** (German conventions): `{prefix}_addon` (Adresszusatz, e.g.
  "c/o Fr. Müller", always optional), `{prefix}_street` (incl. house number, not split out),
  `{prefix}_zip`, `{prefix}_city`. Optional addresses are all-or-none via CHECK:
  street/zip/city set together or all NULL, and addon only when the address is present.
- **Draft rows (auto-save)**: form-edited entities carry `draft BOOLEAN NOT NULL DEFAULT true`.
  Completion-mandatory columns (marked ★ below) are nullable in SQL and enforced via
  `CHECK (draft OR (<★ columns> IS NOT NULL ...))`. The server recomputes `draft` on every
  write — it flips to `false` automatically with the write that fills the mandatory set; there
  is no save button. One-way: clearing a ★ column on a complete row is rejected (422), never
  persisted. See "Draft rows" section below.

## Enums (Postgres types)

| Enum | Values |
|---|---|
| `salutation` | `frau`, `herr`, `familie` (UI labels translated; stored values stable) |
| `packaging_kind` | `original`, `subset` |
| `movement_kind` | `dispense`, `correction` |
| `service_type` | `got`, `self_defined` |
| `template_item_kind` | `drug_packaging`, `service` |
| `treatment_item_kind` | `drug_packaging`, `service` |
| `attachment_kind` | `patient_file`, `treatment_file`, `referenced` |
| `invoice_status` | `created`, `accepted`, `submitted`, `cancelled` |
| `email_type` | `private`, `work`, `other` |

## Draft rows (auto-save)

Draft-enabled tables and their completeness sets (★ columns — nullable in SQL, required by
the completeness CHECK to leave draft state):

| Table | Completeness set (★) |
|---|---|
| customer | salutation, last_name, home_street, home_zip, home_city |
| patient | customer_id, name, sex, species |
| drug | name, manufacturer_id, vat_percent |
| drug_packaging | unit, quantity, list_price_net, sales_price_gross; supplier_id for originals |
| service | name, vat_percent, gross_price; got_number + factor for GOT type |
| supplier / manufacturer | name |
| treatment_template | name |
| appointment | starts_at |

Semantics:
- Kind-bound CHECKs whose columns are filled during form entry are draft-aware:
  e.g. packaging supplier `CHECK (draft OR ((kind = 'original') = (supplier_id IS NOT NULL)))`
  and the service GOT CHECK evaluate only on completion. Structural discriminators chosen at
  creation (packaging `kind`, service `type`, attachment `kind`) stay plain NOT NULL.
- Pickers, treatment lines, stock intake, and invoicing only see `draft = false` rows;
  treatments can only be added to non-draft appointments.
- Lists show drafts flagged "incomplete — missing: …" and they are resumable.
- The nightly job deletes drafts older than 24 h whose ★ fields are all empty and that have
  no children.
- **Not** draft-enabled: treatment (own fields all optional, parent set at creation),
  treatment_item (born complete from the picker), drug_stock_lot/movement (one-shot validated
  actions), invoice (action-generated), global_settings (empty values legitimate).

## Master data

### customer
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| salutation | salutation | ★ |
| first_name | text | NULL |
| last_name | text | ★ |
| second_salutation | salutation | NULL |
| second_first_name | text | NULL |
| second_last_name | text | NULL |
| home_addon | text | NULL |
| home_street | text | ★ (incl. house number) |
| home_zip | text | ★ |
| home_city | text | ★ |
| invoice_salutation | salutation | NULL |
| invoice_first_name | text | NULL |
| invoice_last_name | text | NULL |
| invoice_addon | text | NULL |
| invoice_street | text | NULL |
| invoice_zip | text | NULL |
| invoice_city | text | NULL |
| phone | text | NULL (validated + normalized E.164, e.g. `+493012345678`; national display format in UI) |
| warning_remark | text | NULL |
| archived | boolean | NOT NULL DEFAULT false |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

CHECKs:
- Completeness: `CHECK (draft OR (salutation IS NOT NULL AND last_name IS NOT NULL AND
  home_street IS NOT NULL AND home_zip IS NOT NULL AND home_city IS NOT NULL))`.
- Second name all-or-none: `(second_salutation IS NULL) = (second_last_name IS NULL)`;
  `second_first_name` only when the second name is present.
- Invoice address group all-or-none over `(invoice_salutation, invoice_last_name,
  invoice_street, invoice_zip, invoice_city)`; `invoice_first_name`/`invoice_addon` only when
  the group is present.

Invoice address-block rendering: the invoice recipient (salutation, first name, last name +
invoice address) when the group is set; otherwise the primary name line, the second name as a
second line when present, and the home address. Lists sort by `last_name, first_name`;
customer search matches both names.

### customer_email
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| customer_id | bigint | FK → customer, NOT NULL, ON DELETE CASCADE |
| email | text | NOT NULL |
| email_type | email_type | NOT NULL DEFAULT 'private' |

### patient
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| customer_id | bigint | FK → customer, ★ |
| name | text | ★ |
| sex | text | ★ (de/en labeled select; stored value set: `female`, `male`, `unknown`) |
| species | text | ★ (UI offers Cat/Dog, free text allowed) |
| race | text | NULL |
| colour | text | NULL |
| date_of_birth | date | NULL |
| photo_attachment_id | bigint | FK → attachment, NULL (attachment kind `referenced`) |
| date_of_death | date | NULL |
| chip_number | text | NULL |
| eu_passport_number | text | NULL |
| warning_remark | text | NULL |
| neutered | boolean | NOT NULL DEFAULT false |
| insured | boolean | NOT NULL DEFAULT false |
| archived | boolean | NOT NULL DEFAULT false |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

Rule (application layer): setting `date_of_death` sets `archived = true`.

### supplier / manufacturer
Identical shape: `id` PK, `name text` ★, optional address column group
(`addr_addon`, `addr_street`, `addr_zip`, `addr_city` with the all-or-none CHECK),
`archived`, `draft`.

## Pharmacy

### drug
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| name | text | ★ |
| manufacturer_id | bigint | FK → manufacturer, ★ |
| submission_receipt | boolean | NOT NULL DEFAULT false (informational in v1) |
| narcotic | boolean | NOT NULL DEFAULT false (informational in v1) |
| vaccine | boolean | NOT NULL DEFAULT false |
| refrigerate | boolean | NOT NULL DEFAULT false |
| redesignation | boolean | NOT NULL DEFAULT false |
| vat_percent | numeric(7,3) | ★ (chosen from configured rates; pinned per row) |
| approval_number | text | NULL |
| archived | boolean | NOT NULL DEFAULT false |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

### drug_packaging
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| drug_id | bigint | FK → drug, NOT NULL |
| kind | packaging_kind | NOT NULL (chosen at creation) |
| unit | text | ★ |
| quantity | numeric(10,2) | ★ CHECK (> 0 when set) |
| list_price_net | numeric(10,2) | ★ (entered for original; derived pro-rata for subset) |
| sales_price_gross | numeric(10,2) | ★ (computed per AMPreisV, manually overridable) |
| supplier_id | bigint | FK → supplier, NULL (★ for originals) |
| archived | boolean | NOT NULL DEFAULT false |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

Constraints:
- `UNIQUE (drug_id) WHERE kind = 'original'` — at most one original packaging per drug
  (partial unique index; drafts occupy the slot too).
- `CHECK (draft OR ((kind = 'original') = (supplier_id IS NOT NULL)))` — supplier on originals
  only, evaluated on completion.
- `UNIQUE (id, kind)` — target for the composite FK from `drug_stock_lot`.

### drug_stock_lot
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| packaging_id | bigint | NOT NULL |
| packaging_kind | packaging_kind | NOT NULL CHECK (= 'original') |
| arrival_date | date | NOT NULL DEFAULT current_date |
| packages_received | integer | NOT NULL CHECK (> 0) |
| initial_quantity | numeric(10,2) | NOT NULL (snapshot: packages_received × packaging.quantity at intake) |
| batch_number | text | NULL |
| expiration_date | date | NULL |

Composite FK `(packaging_id, packaging_kind) REFERENCES drug_packaging (id, kind)` — lots can
only reference original packagings, enforced in the DB.
Index: partial index on `expiration_date` (dashboard widget / FEFO).

### drug_stock_movement
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| lot_id | bigint | FK → drug_stock_lot, NOT NULL |
| kind | movement_kind | NOT NULL |
| quantity | numeric(10,2) | NOT NULL, signed |
| moved_at | timestamptz | NOT NULL DEFAULT now() |
| treatment_item_id | bigint | FK → treatment_item, NULL |
| reason | text | NULL |
| reverses_movement_id | bigint | FK → drug_stock_movement, NULL |

CHECKs (kind-bound columns per modeling conventions):
- `kind = 'dispense'` → `quantity < 0 AND treatment_item_id IS NOT NULL AND reason IS NULL AND reverses_movement_id IS NULL`
- `kind = 'correction'` → `treatment_item_id IS NULL` (quantity may be positive or negative;
  `reason` optional; `reverses_movement_id` optional — set for invoice-cancellation reversals)

Semantics (application layer, tested against the real DB):
- **Draft vs frozen is derived**: a dispense is a draft while its treatment has no invoice in
  status `accepted`/`submitted`; drafts are freely deleted/rewritten on treatment edits.
- Once the invoice is Accepted, movements are **append-only** ("Storno statt Löschen"):
  cancellation inserts compensating corrections with `reverses_movement_id` set; nothing is
  updated or deleted.
- **Stock is always derived**: per lot `initial_quantity + SUM(quantity)`; per drug the sum
  over the lots of its original packaging. A `lot_remaining` SQL view backs lists, FEFO
  suggestions, and the dashboard expiry widget. When total stock is insufficient, the
  user-confirmed shortfall dispense is booked against the FEFO-suggested (or user-chosen)
  lot, whose derived remaining may go negative until a correction reconciles it.

## Services & templates

### service
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| type | service_type | NOT NULL (chosen at creation) |
| name | text | ★ |
| got_number | text | NULL (★ for GOT type) |
| factor | numeric(7,3) | NULL, DEFAULT 100 (★ for GOT type) |
| vat_percent | numeric(7,3) | ★ |
| gross_price | numeric(10,2) | ★ (single GOT rate for `got`) |
| travel_expenses | boolean | NOT NULL DEFAULT false |
| hidden | boolean | NOT NULL DEFAULT false |
| archived | boolean | NOT NULL DEFAULT false |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

CHECK (draft-aware, evaluated on completion): `draft OR ((type = 'got' AND got_number IS NOT NULL AND factor IS NOT NULL) OR (type = 'self_defined' AND got_number IS NULL))`.
The GOT 2022 import migration populates `type = 'got'` rows; surgical positions get `hidden = true`.

### treatment_template
`id` PK, `name text` ★, `archived`, `draft`.

### treatment_template_item
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| template_id | bigint | FK → treatment_template, NOT NULL, ON DELETE CASCADE |
| position | integer | NOT NULL; `UNIQUE (template_id, position) DEFERRABLE INITIALLY DEFERRED` |
| kind | template_item_kind | NOT NULL |
| drug_packaging_id | bigint | FK → drug_packaging, NULL |
| service_id | bigint | FK → service, NULL |
| quantity | numeric(10,2) | NOT NULL CHECK (> 0) |
| unit | text | NULL (drug items) |

XOR CHECK: `(kind = 'drug_packaging') = (drug_packaging_id IS NOT NULL) AND (kind = 'service') = (service_id IS NOT NULL)`.

## Appointments & treatments

### appointment
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| starts_at | timestamptz | ★ (minute resolution; date defaults to today in UI, time always user-filled) |
| note | text | NULL |
| draft | boolean | NOT NULL DEFAULT true (see Draft rows) |

### treatment
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| appointment_id | bigint | FK → appointment, NOT NULL |
| treatment_reason | text | NULL |
| finding | text | NULL |

### treatment_patient (n:m)
`treatment_id` FK, `patient_id` FK, `PRIMARY KEY (treatment_id, patient_id)`.

Rule: all patients of a treatment belong to the **same customer** (the invoice's customer is
derived from it). Application-enforced when adding a patient (cross-customer patient → 422),
covered by integration test.

### treatment_item
All billing-relevant values are **pinned at line entry** (copied from catalog; never re-read).

| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| treatment_id | bigint | FK → treatment, NOT NULL, ON DELETE CASCADE |
| position | integer | NOT NULL; `UNIQUE (treatment_id, position) DEFERRABLE INITIALLY DEFERRED` |
| kind | treatment_item_kind | NOT NULL |
| drug_packaging_id | bigint | FK → drug_packaging, NULL (traceability reference, not price source) |
| service_id | bigint | FK → service, NULL (reference) |
| patient_id | bigint | FK → patient, NULL |
| name | text | NOT NULL (copied; override allowed) |
| quantity | numeric(10,2) | NOT NULL CHECK (> 0) |
| unit | text | NULL |
| factor | numeric(7,3) | NULL |
| got_number | text | NULL (copied for GOT services) |
| price_gross | numeric(10,2) | NOT NULL (per-unit gross price; copied, overridable) |
| vat_percent | numeric(7,3) | NOT NULL (copied) |
| km | numeric(10,2) | NULL (travel-expense lines: entered kilometers the price was computed from) |

CHECKs:
- XOR: `(kind = 'drug_packaging') = (drug_packaging_id IS NOT NULL) AND (kind = 'service') = (service_id IS NOT NULL)`
- Patient attribution: `kind = 'drug_packaging' → patient_id IS NOT NULL` (mandatory for drug
  lines, optional for service lines).
- Drug lines: `unit IS NOT NULL AND factor IS NULL AND got_number IS NULL AND km IS NULL`.

Line total = `round(price_gross × quantity × coalesce(factor,100)/100, 2)`; VAT is extracted
from gross per rate for the invoice VAT summary.

## Invoicing

### invoice
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| treatment_id | bigint | FK → treatment, NOT NULL |
| invoice_number | text | NOT NULL, UNIQUE (string — pattern-formatted, never numeric) |
| invoice_date | date | NOT NULL (set at creation; set anew on the replacement invoice created by an update) |
| status | invoice_status | NOT NULL DEFAULT 'created' |
| includes_finding | boolean | NOT NULL DEFAULT false |
| note | text | NULL |
| pdf_attachment_id | bigint | FK → attachment, NULL (kind `referenced`; set when PDF generated) |
| email_recipients | text[] | NULL |
| ts_accepted | timestamptz | NULL |
| ts_sent_email | timestamptz | NULL |
| ts_submitted | timestamptz | NULL |
| ts_cancelled | timestamptz | NULL |

Constraints:
- `UNIQUE (treatment_id) WHERE status <> 'cancelled'` — at most one live invoice per treatment;
  cancelled invoices are retained for bookkeeping.
- Status/timestamp CHECKs: `status = 'accepted' → ts_accepted IS NOT NULL`,
  `status = 'submitted' → ts_submitted IS NOT NULL`, `status = 'cancelled' → ts_cancelled IS NOT NULL`.

State machine (status is the lifecycle, timestamps are the audit record):

```
created ──accept──▶ accepted ──mark submitted──▶ submitted
   │                    │                            │
   └──────cancel────────┴──────────cancel────────────┘──▶ cancelled (terminal)
```

- `created → accepted`: select recipients, email PDF, freeze dispense movements.
- Update flow: `created` keeps its invoice number; updating an `accepted`/`submitted` invoice
  auto-cancels it first (number burned) and creates a new `created` invoice.
- `→ cancelled`: writes compensating stock corrections for all frozen dispenses of the treatment.
- Listings exclude `cancelled` by default.

### invoice_number_sequence
| Column | Type | Constraints |
|---|---|---|
| scope | text | PK (e.g. `"2026"`; derived from the configured pattern) |
| counter | bigint | NOT NULL DEFAULT 0 (monotonic; `UPDATE ... SET counter = counter + 1 RETURNING` under the row lock; never decremented) |

Numbers are never reused: `created` invoices keep their number on update; numbers of
`accepted` invoices are burned on cancellation.

## Files & settings

### attachment
| Column | Type | Constraints |
|---|---|---|
| id | identity | PK |
| sha256 | text | NOT NULL (hex; file lives at `attachments/ab/cd/<sha256>`) |
| mime_type | text | NOT NULL |
| size_bytes | bigint | NOT NULL |
| orig_name | text | NOT NULL |
| kind | attachment_kind | NOT NULL |
| patient_id | bigint | FK → patient, NULL |
| treatment_id | bigint | FK → treatment, NULL |
| reference_date | date | NULL (patient files: date the document refers to) |
| note | text | NULL |

CHECKs per kind: `patient_file → patient_id NOT NULL AND treatment_id IS NULL`;
`treatment_file → treatment_id NOT NULL AND patient_id IS NULL`;
`referenced → patient_id IS NULL AND treatment_id IS NULL` (owner points here: patient photo,
invoice PDF, practice logo).

### global_settings (single row)
| Column | Type | Constraints |
|---|---|---|
| id | boolean | PK DEFAULT true, CHECK (id) — the classic single-row guard |
| practice_name | text | NOT NULL DEFAULT '' |
| practice_address | text | NOT NULL DEFAULT '' |
| iban | text | NOT NULL DEFAULT '' |
| ustid | text | NOT NULL DEFAULT '' |
| logo_attachment_id | bigint | FK → attachment, NULL |
| cc_emails | text[] | NOT NULL DEFAULT '{}' |
| bcc_emails | text[] | NOT NULL DEFAULT '{}' |

### picker_usage (derived, nightly refresh)
| Column | Type | Constraints |
|---|---|---|
| kind | treatment_item_kind | part of PK |
| item_id | bigint | part of PK (drug_packaging.id or service.id) |
| uses | bigint | NOT NULL |

Rebuilt nightly from `treatment_item` counts; read by the unified picker for ranking.

### sessions
Managed by `tower-sessions` Postgres store (its own migration).

## Views

- `lot_remaining`: `lot_id, packaging_id, drug_id, batch_number, expiration_date, remaining`
  (= `initial_quantity + COALESCE(SUM(movements.quantity), 0)`). Backs stock lists, FEFO
  suggestion (`ORDER BY expiration_date NULLS LAST, id` among `remaining > 0`), and the
  dashboard top-5-expiring widget.
