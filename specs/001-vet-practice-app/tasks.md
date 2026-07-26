---

description: "Task list for Vet Practice Management Application"
---

# Tasks: Vet Practice Management Application

**Input**: Design documents from `/specs/001-vet-practice-app/`

**Prerequisites**: plan.md, spec.md, data-model.md, contracts/rest-api.md, research.md, quickstart.md

**Tests**: MANDATORY per Constitution Principle II — every story phase carries test tasks;
write them first and watch them fail before implementing.

**Organization**: Tasks are grouped by user story (US1–US6 from spec.md) so each story is an
independently testable increment. Story prerequisites (e.g. US1 needs a customer and a stocked
drug) come from seeded test fixtures until the owning story's UI exists.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US6)
- Include exact file paths in descriptions

## Path Conventions

- Backend: `k-vet-backend/` (Axum + sqlx; `src/api/` handlers, `src/domain/` pure logic,
  `migrations/`, `tests/` integration)
- Frontend: `k-vet-web/` (`src/features/`, `src/components/`, `src/lib/`, `src/api/generated/`)
- E2E: `e2e/` (Playwright, desktop + pixel-9a projects)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and tooling per plan.md / research.md

- [ ] T001 Populate `k-vet-backend/Cargo.toml` with pinned full-semver dependencies: axum, tokio, sqlx (postgres, rustls, migrate, rust_decimal), utoipa, tower-http, tower-sessions + postgres store, rust_decimal, argon2, lettre (rustls), typst, rust-embed, image, serde, toml, thiserror, tracing, tracing-subscriber (env-filter + chrono features)
- [ ] T002 [P] Create `docker-compose.yml` at repo root: Postgres service for dev and local tests (per quickstart.md)
- [ ] T003 [P] Rework `k-vet-web/package.json`: drop oxlint; add biome, knip, vitest, @testing-library/react, msw, orval, @tanstack/react-query, @tanstack/react-table, @tanstack/react-router, react-hook-form, zod, tailwindcss, shadcn/ui deps, i18next, react-i18next, cmdk — full-semver pins; scripts for dev/build/test/lint/generate:api
- [ ] T004 [P] Add lint/format configs: `k-vet-web/biome.json`, `k-vet-web/knip.json`, strict `k-vet-web/tsconfig.json`; `k-vet-backend/rustfmt.toml` and clippy lint level (deny warnings) in `k-vet-backend/Cargo.toml`
- [ ] T005 [P] Write `config.example.toml` at repo root with bilingual (de-DE + en-US) comments: user + argon2 password hash, DB URL, listen address/port, log_level, base URL, SMTP, invoice number pattern, currency, VAT rate choices, Wegegeld rates, attachments dir, Typst/email template overrides (research R13)
- [ ] T006 [P] Implement config loader `k-vet-backend/src/config.rs` (serde + toml, `KVET_CONFIG` env override, no unwrap/expect) with unit tests for parse errors and defaults
- [ ] T007 [P] Scaffold `e2e/` Playwright project: `e2e/playwright.config.ts` with `desktop` and `pixel-9a` viewport projects, `e2e/global-setup.ts` spawning the built binary against Postgres
- [ ] T008 [P] Create `.github/workflows/ci.yml` per testing.md: backend job (fmt --check, clippy -D warnings, sqlx prepare --check, cargo test with Postgres service), frontend job (biome, knip, tsc --noEmit, vitest, codegen drift check), e2e job (build binary + Playwright both projects), tag-release job (cargo-zigbuild aarch64), rust-cache/npm/Playwright caches

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Complete DB schema (all entities serve multiple stories), auth, contract
pipeline, and frontend/auto-save infrastructure — MUST be complete before ANY user story

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T009 Migration `k-vet-backend/migrations/0001_enums.sql`: all enums from data-model.md (salutation, packaging_kind, movement_kind, service_type, template_item_kind, treatment_item_kind, attachment_kind, invoice_status, email_type) + `updated_at` trigger function; enable `pg_trgm`
- [ ] T010 Migration `0002_master_data.sql`: customer (structured names, second-name CHECK, home/invoice address groups, phone, draft + completeness CHECK), customer_email, patient (draft CHECK), supplier, manufacturer
- [ ] T011 Migration `0003_pharmacy.sql`: drug, drug_packaging (partial unique original, UNIQUE(id,kind), draft-aware supplier CHECK), drug_stock_lot (composite FK to original), drug_stock_movement (kind-bound CHECKs), `lot_remaining` view, expiration partial index
- [ ] T012 Migration `0004_services_templates.sql`: service (draft-aware GOT CHECK), treatment_template, treatment_template_item (XOR CHECK, deferrable position unique)
- [ ] T013 Migration `0005_treatments.sql`: appointment (draft), treatment, treatment_patient, treatment_item (XOR, drug-line patient CHECK, deferrable position unique); migration `0006_invoices.sql`: invoice (status/timestamp CHECKs, one-live-invoice partial unique), invoice_number_sequence
- [ ] T014 Migration `0007_files_settings.sql`: attachment (kind CHECKs), global_settings (single-row guard), picker_usage; tower-sessions store migration
- [ ] T015 Implement `k-vet-backend/src/error.rs` (AppError → RFC 7807 problem+json incl. 422 field errors, errors logged with context) and `k-vet-backend/src/main.rs` skeleton: router assembly, AppState (PgPool + Config), tracing-subscriber init (EnvFilter from config `log_level` with `RUST_LOG` override, ChronoLocal human-readable local-time timestamps per R16), tower-http `TraceLayer` request logging, `/healthz`, `/metrics` stub
- [ ] T016 Auth: `k-vet-backend/src/auth.rs` — argon2 verification against config credentials, tower-sessions Postgres store, `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/session`, auth middleware for `/api/*`; integration test `k-vet-backend/tests/auth.rs` (#[sqlx::test]: wrong password 401, session persists, logout invalidates)
- [ ] T017 Draft-row helper `k-vet-backend/src/domain/draft.rs`: per-entity completeness recompute-on-write (research R15); test `k-vet-backend/tests/draft.rs` proving the completeness CHECK rejects `draft = false` with missing fields and the one-way transition (clearing a mandatory field → 422)
- [ ] T018 Attachments core: `k-vet-backend/src/domain/files.rs` (stream-to-temp while hashing → atomic rename to `attachments/ab/cd/<sha256>`, thumbnail generation, dedup) + `k-vet-backend/src/api/attachments.rs` (upload/stream/thumbnail, immutable cache headers); test `k-vet-backend/tests/attachments.rs`
- [ ] T019 Contract pipeline: `k-vet-backend/src/bin/export-openapi.rs` (utoipa document), committed `k-vet-backend/openapi.json`, `k-vet-web/orval.config.ts` emitting TanStack Query hooks + zod to `k-vet-web/src/api/generated/`, drift-check script `scripts/check-codegen-drift.sh` (referenced by CI)
- [ ] T020 Frontend shell: `k-vet-web/src/main.tsx`, TanStack Router root layout + nav in `k-vet-web/src/routes/`, query client in `k-vet-web/src/lib/query.ts`, i18n setup `k-vet-web/src/lib/i18n.ts` + `k-vet-web/src/i18n/{de,en}.json` (de-DE default), login route + auth guard
- [ ] T021 Auto-save infrastructure: `k-vet-web/src/lib/autosave.ts` (600 ms debounced PATCH, flush on blur/pagehide, optimistic cache update), `k-vet-web/src/components/SaveIndicator.tsx`, draft-aware form helper (shows "incomplete — missing: …"); vitest tests in `k-vet-web/tests/autosave.test.tsx`
- [ ] T022 [P] Responsive list component `k-vet-web/src/components/DataList.tsx`: TanStack Table rendering table on desktop, stacked cards on mobile; vitest test `k-vet-web/tests/datalist.test.tsx`
- [ ] T023 [P] Shared test fixtures: `k-vet-backend/tests/common/mod.rs` (seed customer/patient/drug + original packaging + lot/service/template) and `e2e/fixtures/seed.ts` (API-driven seeding for story-independent E2E)

**Checkpoint**: Foundation ready — schema, auth, contracts, shell, auto-save all in place

---

## Phase 3: User Story 1 - Record a Treatment and Issue an Invoice (Priority: P1) 🎯 MVP

**Goal**: The core daily loop — appointment → treatment with pinned-price lines and FEFO
draft dispenses → invoice Created → PDF inspection → Accept + email, with the update/cancel
number-burning rules.

**Independent Test**: With fixture-seeded customer/patient/service/stocked drug: record a
treatment with two lines, create the invoice, accept it, verify PDF + email hand-off + frozen
stock movements.

### Tests for User Story 1 (MANDATORY) ⚠️

> Write these FIRST, ensure they FAIL before implementation

- [ ] T024 [P] [US1] Failing unit tests for money math (VAT extraction per rate, line total = price × qty × factor/100, half-up rounding) as test module in `k-vet-backend/src/domain/money.rs`
- [ ] T025 [P] [US1] Failing unit tests for invoice number pattern + scope derivation + monotonic allocation in `k-vet-backend/src/domain/invoice_number.rs`
- [ ] T026 [P] [US1] Failing integration tests: item pinning (catalog change never alters lines), FEFO draft dispenses + recalc on edit, freeze on accept, compensating corrections + burned number on cancel/update in `k-vet-backend/tests/treatments.rs` and `k-vet-backend/tests/invoices.rs`

### Implementation for User Story 1

- [ ] T027 [US1] Implement `k-vet-backend/src/domain/money.rs` (rust_decimal; VAT extraction, line totals, factor application) — T024 green
- [ ] T028 [US1] Implement `k-vet-backend/src/domain/invoice_number.rs` (config pattern, per-scope row-locked counter, never reuse) — T025 green
- [ ] T029 [US1] Implement `k-vet-backend/src/domain/stock.rs`: FEFO suggestion over `lot_remaining`, split across lots, draft recalculation, freeze on accept, compensating corrections linked via `reverses_movement_id`
- [ ] T030 [US1] Appointments API `k-vet-backend/src/api/appointments.rs` (CRUD, draft/starts_at rules, duplicate with `price_mode`) + integration test `k-vet-backend/tests/appointments.rs`
- [ ] T031 [US1] Treatments API `k-vet-backend/src/api/treatments.rs` (create under non-draft appointment, PATCH reason/finding, patients set, duplicate, files) 
- [ ] T032 [US1] Treatment items API `k-vet-backend/src/api/treatment_items.rs`: add with server-side pinning (name/price/VAT/factor/GOT number), patient preselect rule, PATCH (recalc drafts), DELETE, `/move`, `/lots` FEFO override — T026 treatments tests green
- [ ] T033 [US1] Picker endpoint `k-vet-backend/src/api/picker.rs` (pg_trgm search over packagings + services, `usage DESC, similarity DESC`, tagged union, excludes hidden/archived/draft) with an integration timing assertion (ranked results < 1 s on a seeded full catalog — SC-003), and `POST /api/treatments/{id}/apply-template` in `k-vet-backend/src/api/treatments.rs`
- [ ] T034 [US1] Invoice PDF: `k-vet-backend/src/pdf/mod.rs` with embedded `invoice.typ` (address block: invoice recipient else customer name(s) two-line + home address; line items, VAT summary, IBAN/UStID/logo from settings; config template override) + snapshot test
- [ ] T035 [US1] Mail `k-vet-backend/src/mail/mod.rs` (lettre + rustls, CC/BCC from global settings, sent-timestamp only on success); invoices API `k-vet-backend/src/api/invoices.rs`: create/update from treatment (Created keeps number, Accepted auto-cancels + burns), accept (recipients, freeze via domain/stock, email PDF), PDF streaming — T026 invoices tests green
- [ ] T036 [P] [US1] Frontend appointments feature `k-vet-web/src/features/appointments/` (list via DataList, detail, duplicate dialog with price mode choice)
- [ ] T037 [US1] Frontend treatment page `k-vet-web/src/features/treatments/`: line editor with unified picker (cmdk async combobox, drug/service icons), reorder controls, per-line patient attribution with single-patient preselect, price/name overrides, apply-template, auto-save wiring
- [ ] T038 [US1] Frontend invoice flow `k-vet-web/src/features/treatments/invoice/`: create dialog (Includes Finding, note), PDF preview, accept dialog with recipient multi-select, update/auto-cancel messaging
- [ ] T039 [US1] E2E `e2e/tests/core-loop.spec.ts` (quickstart #1), `e2e/tests/autosave.spec.ts` (#2 incl. reload mid-typing), `e2e/tests/invoice-numbers.spec.ts` (#6) — both viewport projects

**Checkpoint**: MVP — the practice can document and bill a visit end to end

---

## Phase 4: User Story 2 - Manage Customers and Patients (Priority: P2)

**Goal**: Master data with structured names/addresses, contact validation, warnings,
archiving, patient photos and files.

**Independent Test**: Create a customer (two emails, phone, warning), add a patient with
photo/species/files, verify warning display, validation, archiving and search behavior.

### Tests for User Story 2 (MANDATORY) ⚠️

- [ ] T040 [P] [US2] Failing unit tests for contact validation (email syntax; DE phone parse "0171 / 123 45 67" → `+49171123456` E.164 + national display; international input; rejects) as test module in `k-vet-backend/src/domain/contact.rs`
- [ ] T041 [P] [US2] Failing integration tests: archive/unarchive + hidden-from-search default, death-date auto-archive, invalid email/phone → 422 + last valid kept, draft completion flip in `k-vet-backend/tests/customers.rs` and `k-vet-backend/tests/patients.rs`

### Implementation for User Story 2

- [ ] T042 [US2] Implement `k-vet-backend/src/domain/contact.rs` (email_address + phonenumber crates per R14) — T040 green
- [ ] T043 [US2] Customers API `k-vet-backend/src/api/customers.rs` (CRUD/PATCH auto-save with draft recompute, emails sub-resource, archive/unarchive, search matching both names, phone normalization) — T041 customers tests green
- [ ] T044 [US2] Patients API `k-vet-backend/src/api/patients.rs` (CRUD, photo attachment reference, patient files with reference date + note, death-date auto-archive) — T041 patients tests green
- [ ] T045 [P] [US2] Warning components `k-vet-web/src/components/WarningBanner.tsx` (prominent on open) + warning icon/tooltip cell for DataList
- [ ] T046 [US2] Customers UI `k-vet-web/src/features/customers/`: responsive list, detail form (salutation/first/last name, optional second name, home + optional invoice address with recipient name, typed emails editor, phone field with validation error display)
- [ ] T047 [US2] Patients UI `k-vet-web/src/features/patients/`: photo circle, species combobox (Cat/Dog defaults + free text), files upload with reference date/note, warning display, death-date archiving feedback
- [ ] T048 [US2] E2E `e2e/tests/master-data.spec.ts` (quickstart #11 + validation: invalid email/phone rejected with error, national phone format redisplay, archived hidden until toggled)

**Checkpoint**: US1 + US2 work independently — master data no longer fixture-only

---

## Phase 5: User Story 3 - Run the Pharmacy: Drugs, Prices, and Stock (Priority: P2)

**Goal**: Drug/packaging management with AMPreisV pricing, lot-based stock with FEFO,
corrections, and batch traceability.

**Independent Test**: Create drug + original/subset packagings, verify computed prices,
record intake, dispense via treatment, correct stock, read the lot's chronological history.

### Tests for User Story 3 (MANDATORY) ⚠️

- [ ] T049 [P] [US3] Failing AMPreisV worked-example unit tests in `k-vet-backend/src/domain/money.rs` test module: original packaging surcharge chain (§ 10 → § 3(1) S.2–3, § 3(2)–(4)) and subset § 4 Teilmengenzuschlag — exact tiers transcribed from https://www.gesetze-im-internet.de/ampreisv/ (R11), examples flagged for vet review
- [ ] T050 [P] [US3] Failing integration tests: movement kind CHECKs, derived stock reconciliation after intake/dispense/correction/cancel, append-only after accept, correction default = remaining, reasons list in `k-vet-backend/tests/stock.rs`

### Implementation for User Story 3

- [ ] T051 [US3] Implement AMPreisV in `k-vet-backend/src/domain/money.rs` + `POST /api/pricing/preview` in `k-vet-backend/src/api/pricing.rs` — T049 green
- [ ] T052 [P] [US3] Suppliers + manufacturers APIs `k-vet-backend/src/api/suppliers.rs`, `k-vet-backend/src/api/manufacturers.rs` (CRUD, structured optional address, archive) + tests in `k-vet-backend/tests/suppliers.rs`
- [ ] T053 [US3] Drugs + packagings APIs `k-vet-backend/src/api/drugs.rs`, `k-vet-backend/src/api/packagings.rs` (flags informational, VAT selector values from config, server-computed sales price unless overridden, original/subset + draft rules)
- [ ] T054 [US3] Stock APIs `k-vet-backend/src/api/lots.rs`: `POST /api/packagings/{id}/stock-intakes` (initial-quantity snapshot), lot list with remaining, lot detail with movement links to treatment/invoice/customer, corrections + `GET /api/stock/correction-reasons` — T050 green
- [ ] T055 [P] [US3] Pharmacy UI `k-vet-web/src/features/pharmacy/`: drug list/detail with flags, packaging editor with live price preview and override
- [ ] T056 [US3] Stock UI `k-vet-web/src/features/pharmacy/stock/`: intake form (defaults today), lot list with derived remaining, lot detail traceability view, correction dialog (default remaining, editable reason select alphabetical + new)
- [ ] T057 [US3] E2E `e2e/tests/pharmacy.spec.ts` (quickstart #7 intake → FEFO split dispense → accept → cancel → stock equals initial again; #4 batch traceability links)

**Checkpoint**: Pharmacy fully operational, prices computed per AMPreisV

---

## Phase 6: User Story 4 - Service Catalog and Treatment Templates (Priority: P3)

**Goal**: GOT 2022 catalog imported (surgical hidden), self-defined services, travel-expense
km computation, reusable ordered templates.

**Independent Test**: GOT positions searchable with surgical hidden, create self-defined
service, travel line computes from km, template items reorder and apply in order.

### Tests for User Story 4 (MANDATORY) ⚠️

- [ ] T058 [P] [US4] Failing Wegegeld unit tests in `k-vet-backend/src/domain/money.rs` test module: `max(km × 3.50, 13.00)`, ×1–3 multiplier, config-driven rates, VAT applies (GOT 2022 § 10 per R10)
- [ ] T059 [P] [US4] Failing integration tests: hidden-by-default filtering, GOT CHECK on completion, template item ordering ops, apply-template pins current prices in order in `k-vet-backend/tests/services.rs` and `k-vet-backend/tests/templates.rs`

### Implementation for User Story 4

- [ ] T060 [US4] Implement Wegegeld in `k-vet-backend/src/domain/money.rs` + km field flow on travel-expense lines (`k-vet-backend/src/api/treatment_items.rs`, km input in `k-vet-web/src/features/treatments/`) — T058 green
- [ ] T061 [US4] GOT 2022 extraction: `scripts/extract-got.py` parsing `requirements/GOT_2022.pdf` Gebührenverzeichnis → hand-reviewed migration `k-vet-backend/migrations/0008_got_import.sql` (number, name, single rate, VAT, surgical positions `hidden = true`) with spot-check verification notes
- [ ] T062 [US4] Services API `k-vet-backend/src/api/services.rs` (GOT/self-defined CRUD, hidden default filter, factor rules) — T059 services tests green
- [ ] T063 [US4] Templates API `k-vet-backend/src/api/templates.rs` (items CRUD, `/move` up/down/top/bottom with deferred unique positions) — T059 templates tests green
- [ ] T064 [P] [US4] Services UI `k-vet-web/src/features/services/` (list with hidden toggle + GOT badge, editor with factor/VAT/travel flag)
- [ ] T065 [US4] Templates UI `k-vet-web/src/features/templates/` (item list with reorder controls, drug/service picker)
- [ ] T066 [US4] E2E `e2e/tests/services-templates.spec.ts` (GOT searchable/surgical hidden, km travel line on invoice, template apply order + current prices)

**Checkpoint**: Catalog complete — treatment entry fast via GOT + templates

---

## Phase 7: User Story 5 - Invoice Overview and Bookkeeping Hand-off (Priority: P3)

**Goal**: Invoice list with lifecycle actions and the two-click monthly bookkeeping hand-off.

**Independent Test**: With several accepted invoices: list ordering, bulk ZIP download of
pending PDFs, bulk mark submitted.

### Tests for User Story 5 (MANDATORY) ⚠️

- [ ] T067 [P] [US5] Failing integration tests: list ordering (accepted-not-submitted first, cancelled excluded by default), cancel/submit transitions + timestamps, bulk-submit, pending-PDFs ZIP content in `k-vet-backend/tests/invoice_admin.rs`

### Implementation for User Story 5

- [ ] T068 [US5] Invoice admin endpoints in `k-vet-backend/src/api/invoices.rs`: list with ordering/filters, `POST /{id}/cancel`, `POST /{id}/submit`, `POST /bulk-submit`, `GET /pending-pdfs` (ZIP stream) — T067 green
- [ ] T069 [US5] Invoices page `k-vet-web/src/features/invoices/`: pending section on top, view/download PDF, cancel with confirmation, mark submitted, bulk download + bulk mark actions
- [ ] T070 [US5] E2E `e2e/tests/bookkeeping.spec.ts` (quickstart #8: 3 accepted invoices → bulk download all → bulk submit → pending count 0)

**Checkpoint**: Bookkeeping hand-off works end to end

---

## Phase 8: User Story 6 - Dashboard and Practice Settings (Priority: P4)

**Goal**: Landing dashboard (pending bookkeeping count, top-5 expiring lots) and the
practice settings page.

**Independent Test**: Dashboard counts/links correct; settings edits appear on the next
generated invoice.

### Tests for User Story 6 (MANDATORY) ⚠️

- [ ] T071 [P] [US6] Failing integration tests: dashboard payload (unsubmitted count, top-5 expiring lots with stock via `lot_remaining`), settings single-row PATCH + logo reference in `k-vet-backend/tests/dashboard.rs`

### Implementation for User Story 6

- [ ] T072 [US6] Dashboard API `k-vet-backend/src/api/dashboard.rs` + settings API `k-vet-backend/src/api/settings.rs` (auto-save PATCH, logo upload reference) — T071 green
- [ ] T073 [P] [US6] Dashboard UI `k-vet-web/src/features/dashboard/` (count widget linking to invoices, expiring-lots list linking to lot details)
- [ ] T074 [US6] Settings UI `k-vet-web/src/features/settings/` (practice name/address, IBAN, UStID, logo upload, CC/BCC lists with email validation)
- [ ] T075 [US6] E2E `e2e/tests/dashboard-settings.spec.ts` (widgets navigate correctly; changed IBAN appears on next invoice PDF)

**Checkpoint**: All user stories independently functional

---

## Phase 9: Polish & Cross-Cutting Concerns

- [ ] T076 [P] Scheduled jobs `k-vet-backend/src/jobs/mod.rs`: nightly picker_usage refresh (03:00), nightly draft cleanup (>24 h, all completeness fields empty, no children), monthly attachment orphan sweep — with `#[sqlx::test]` coverage in `k-vet-backend/tests/jobs.rs`
- [ ] T077 [P] Single-binary serving `k-vet-backend/src/static_assets.rs`: rust-embed of `k-vet-web/dist`, SPA fallback, build integration for release profile
- [ ] T078 [P] Theme: extract the practice website's color palette into Tailwind/shadcn theme tokens in `k-vet-web/src/index.css`
- [ ] T079 [P] Prometheus metrics layer wiring in `k-vet-backend/src/main.rs` (request metrics on `/metrics` for the existing scrape)
- [ ] T080 i18n completeness pass: every user-facing string in `k-vet-web/src/i18n/{de,en}.json` (grep for hardcoded strings), de-DE default verified, `config.example.toml` comments bilingual
- [ ] T081 [P] Hardening: request/upload size limits, session cookie flags (HttpOnly/SameSite/Secure), login rate limiting in `k-vet-backend/src/auth.rs` and `k-vet-backend/src/main.rs`
- [ ] T082 Release verification: `cargo zigbuild --target aarch64-unknown-linux-gnu --release` with embedded frontend; run all quickstart.md validation scenarios against the release binary (quickstart #12)
- [ ] T083 [P] Repo `README.md`: dev setup, commands, contract-regeneration workflow (from quickstart.md)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies
- **Foundational (Phase 2)**: depends on Setup — BLOCKS all user stories
- **User Stories (Phases 3–8)**: all depend only on Foundational; mutually independent
  (fixtures provide cross-story data). Suggested order = priority: US1 → US2 → US3 → US4 → US5 → US6
- **Polish (Phase 9)**: after desired stories are complete (T076 draft-cleanup after any
  draft-enabled story; T082 last)

### Cross-story integration notes (don't break independence)

- US4's Wegegeld/km flow (T060) extends US1's treatment-item editor — additive, behind the
  travel-expense service flag.
- US5 extends `src/api/invoices.rs` created in US1 — new endpoints only.
- `domain/money.rs` grows per story (US1 VAT/totals → US3 AMPreisV → US4 Wegegeld); unit
  tests accumulate in the same module.

### Within Each User Story

- Test tasks MUST be written and FAIL before their implementation tasks
- Domain (pure logic) → API handlers → UI → E2E
- Story complete only when its E2E passes in both viewport projects

### Parallel Opportunities

- Phase 1: T002–T008 all [P] after T001
- Phase 2: migrations T009–T014 sequential (numbered files); T020–T023 parallel with backend
  T015–T019
- Within stories: all [P]-marked test tasks together; backend vs frontend tracks
  (e.g. T036 ∥ T030–T035; T055 ∥ T053–T054)
- After Foundational: different stories can proceed in parallel (single developer: priority
  order)

## Parallel Example: User Story 1

```bash
# Write all failing tests first, in parallel:
Task: "T024 money math unit tests in src/domain/money.rs"
Task: "T025 invoice number unit tests in src/domain/invoice_number.rs"
Task: "T026 pinning/FEFO/freeze integration tests in tests/treatments.rs, tests/invoices.rs"

# Then domain modules in parallel:
Task: "T027 domain/money.rs"   Task: "T028 domain/invoice_number.rs"   Task: "T029 domain/stock.rs"

# Frontend track in parallel with API track:
Task: "T036 features/appointments/" ∥ "T030–T035 backend APIs"
```

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 Setup → Phase 2 Foundational (blocks everything)
2. Phase 3 US1 → **STOP and VALIDATE**: core loop E2E green in both viewports; vet can
   document + bill a visit using seeded master data
3. Deploy to the Pi if desired (T077/T082 can be pulled forward for a smoke deploy)

### Incremental Delivery

1. +US2 → real customer/patient management replaces fixtures
2. +US3 → pharmacy + AMPreisV pricing live
3. +US4 → GOT catalog + templates + travel expenses
4. +US5 → bookkeeping hand-off
5. +US6 → dashboard + settings
6. Phase 9 polish → production deployment on the Pi

Each story adds value without breaking previous stories.

---

## Notes

- [P] = different files, no dependency on an incomplete task
- Every task cites concrete file paths; generated artifacts (openapi.json,
  src/api/generated/) are committed and drift-checked in CI
- Verify tests fail before implementing (Constitution II)
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently
