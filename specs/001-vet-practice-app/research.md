# Phase 0 Research: Vet Practice Management Application

**Date**: 2026-07-26 | **Spec**: [spec.md](spec.md)

Most stack-level decisions were made upfront in `requirements/vet-practice-webapp-tech-decisions.md`
and `requirements/testing.md`; they are consolidated here (Decision/Rationale/Alternatives) together
with the decisions that were still open. No NEEDS CLARIFICATION items remain.

## Stack decisions (carried over from requirements, binding per constitution)

### Backend: Rust with Axum + sqlx
- **Decision**: Axum on the Tokio stack, sqlx with compile-time-checked SQL, single static binary.
- **Rationale**: Deployment shape on the Raspberry Pi — one aarch64 binary, ~15 MB RSS, no JVM.
  Compile-time SQL checking pushes schema drift to build time.
- **Alternatives considered**: Kotlin/Spring (rejected: JVM footprint on the Pi), SeaORM
  (fallback if a JPA-like entity layer becomes necessary; start with plain sqlx).

### Database: PostgreSQL on USB SSD
- **Decision**: PostgreSQL; data directory and attachments on the USB SSD, never the SD card.
- **Rationale**: WAL write patterns destroy SD cards; Postgres wins over SQLite on ad-hoc query
  tooling for record data. The datamodel deliberately pushes invariants into the DB
  (CHECK constraints, partial unique indexes, composite FKs).
- **Alternatives considered**: SQLite (viable single-user but weaker tooling).

### API contract: utoipa → OpenAPI → generated TS client
- **Decision**: utoipa annotations on Axum handlers produce the OpenAPI spec; the spec and the
  generated TypeScript client are committed; CI regenerates and fails on diff.
- **Rationale**: Validation rules live once, in Rust; contract drift becomes a build failure.
- **Alternatives considered**: ts-rs (simpler but no hooks/zod generation).

### Frontend: Vite + React, shadcn/ui + Tailwind
- **Decision**: Vite + React 19, shadcn/ui + Tailwind; TanStack Table (headless) rendering a
  table on desktop and stacked cards on mobile; react-hook-form + zod on form-heavy screens;
  debounced async combobox (shadcn/cmdk) for the owner/animal/drug/service pickers.
- **Rationale**: The responsive table→cards split is why headless beats packaged DataTables;
  the picker is the highest-frequency interaction in the app.
- **Alternatives considered**: Mantine/PrimeVue DataTables (rejected: responsive split),
  Dioxus/Leptos (rejected: ecosystem gap on tables/pickers/forms).

### State management
- **Decision**: TanStack Query for all server state; filters/search/current record in URL state;
  react-hook-form for forms; `useState` for odd toggles. No global client-state library.
- **Rationale**: Server cache + URL is sufficient for a single-user CRUD app (YAGNI,
  Constitution V).
- **Alternatives considered**: Zustand (only if genuinely global client-only state emerges).

### Attachments: content-addressed filesystem storage
- **Decision**: Files at `attachments/ab/cd/<sha256>`, metadata in Postgres; upload streams to a
  temp file while hashing → atomic rename → insert row; thumbnails generated at upload
  (`image` crate), also content-addressed; served with `Cache-Control: immutable`; monthly
  orphan-file sweep.
- **Rationale**: Avoids WAL amplification and bloated pg_dump; streaming via tower-http
  `ServeDir`; dedup for free.
- **Alternatives considered**: bytea in Postgres (rejected for the above).

### Invoice PDFs: Typst as embedded Rust library
- **Decision**: `typst` crate embedded in the binary; default `.typ` template compiled in,
  optional file-path override via config.
- **Rationale**: No headless Chrome or external tools on the Pi; typesetting from structured data.
- **Alternatives considered**: HTML→PDF via headless browser (rejected: Pi footprint).

### Testing & CI
- **Decision**: Per `requirements/testing.md` — real Postgres via `#[sqlx::test]` (no mocks, no
  testcontainers); Axum tested in-process; Vitest + RTL + MSW for components; Playwright
  full-stack E2E (desktop + Pixel 9a projects); three GitHub Actions jobs (backend, frontend,
  e2e); `.sqlx` offline metadata committed with `cargo sqlx prepare --check` in CI.
- **Rationale**: The DB holds the invariants, so tests must exercise the real DB; E2E is what
  proves auto-save, PDF generation, and stock side effects.

### Deployment
- **Decision**: One executable — built frontend embedded via rust-embed, API + static assets on
  one port; cross-compiled with cargo-zigbuild for aarch64; rustls throughout (no openssl);
  systemd unit; existing Prometheus monitoring and borgmatic backups extended.
- **Rationale**: Simplest possible operational footprint on the Pi (Constitution V).

## Decisions made in this phase

### R1. TypeScript client generation: orval
- **Decision**: `orval` generating TanStack Query hooks + zod schemas from the committed
  OpenAPI spec, output to `k-vet-web/src/api/generated/` (committed; CI drift-checked).
- **Rationale**: One generator produces both hooks and zod schemas — fewer moving parts than
  combining `openapi-react-query` + `openapi-zod-client`.
- **Alternatives considered**: `openapi-react-query` + `openapi-zod-client` (two tools),
  `ts-rs` (no hooks/zod).

### R2. Authentication & session
- **Decision**: Username + argon2 password hash in the config file; login form → server-side
  session via `tower-sessions` with its Postgres store; session cookie (HttpOnly, SameSite=Lax,
  Secure behind TLS). No user management UI.
- **Rationale**: Single user from configuration per `requirements/general.md`; Postgres-backed
  sessions survive binary restarts (a Pi reboot must not log the vet out mid-day).
- **Alternatives considered**: JWT (needless statelessness for one user), in-memory sessions
  (lost on restart), basic auth (no logout/session semantics, poor mobile UX).

### R3. i18n: react-i18next + Intl formatting/parsing
- **Decision**: `i18next` + `react-i18next`, JSON resource files `de-DE` (default) and `en-US`;
  language switch persisted; no hardcoded user-facing strings (enforced in review).
  i18n covers locale-aware **formatting and parsing**, not just translations:
  `k-vet-web/src/lib/format.ts` wraps `Intl.NumberFormat`/`Intl.DateTimeFormat` driven by the
  active i18next locale (money `1.234,56 €` vs `€1,234.56`, dates `dd.MM.yyyy` vs `M/d/yyyy`),
  plus locale-aware **parse** functions and a numeric input component that accepts decimal
  comma in de-DE / decimal point in en-US. The API wire format stays locale-independent
  (ISO 8601 dates/timestamps, dot-decimal numeric strings) — formatting never crosses the
  contract boundary. Invoice PDFs/emails always format de-DE (Typst template), regardless of
  UI locale.
- **Rationale**: De-facto standard, works with Vite code-splitting; de-DE default per
  constitution Principle III.
- **Alternatives considered**: FormatJS/react-intl (heavier message extraction workflow),
  Lingui (smaller ecosystem).

### R4. Auto-save pattern
- **Decision**: Entities are created as real rows immediately when a "new X" screen opens, so
  every edit has an id to PATCH. react-hook-form `watch` → 600 ms debounce → PATCH mutation
  (TanStack Query) with optimistic cache update; a visible saving/saved indicator; `flush` on
  blur/navigation (`visibilitychange`/`pagehide`). Server PATCHes are partial (only changed
  fields). Playwright E2E asserts persistence across reload (SC-002). Mandatory fields are
  reconciled with immediate creation via draft rows — see R15.
- **Rationale**: "Auto-save all user input" is the top UX requirement; PATCH-with-id avoids
  create/update races; debounce keeps write volume trivial for one user.
- **Alternatives considered**: localStorage draft + explicit commit (violates "no explicit
  save"), full-document PUT (clobbers concurrent tab edits more than field-level PATCH).

### R5. In-process scheduled jobs
- **Decision**: Tokio background tasks inside the same binary: nightly picker-usage-weight
  refresh (03:00 local), nightly cleanup of abandoned all-empty draft rows (R15), and monthly
  attachment orphan sweep. No external cron/services.
- **Rationale**: Constitution V — no runtime services beyond PostgreSQL; the jobs are simple
  SQL/filesystem passes.
- **Alternatives considered**: systemd timers (second deployment artifact to keep in sync),
  `tokio-cron-scheduler` crate (fine too, but a plain interval/target-time loop is enough).

### R6. Email: lettre with rustls
- **Decision**: `lettre` (async, rustls TLS) against the SMTP server from config; global CC/BCC
  from Global Settings applied at send time; sent-timestamp recorded only on SMTP success.
- **Rationale**: Standard Rust mailer; rustls keeps aarch64 cross-builds painless.
- **Alternatives considered**: shelling out to sendmail (external dependency on the Pi).

### R7. Money & percentages representation
- **Decision**: `NUMERIC(10,2)` for money and `NUMERIC(7,3)` for factors/percentages in
  Postgres, `rust_decimal::Decimal` in Rust; rounding half-up to cents at defined points
  (documented next to each formula). All computed prices remain overridable; every Treatment
  Item pins its own price/VAT copy.
- **Rationale**: AMPreisV/GOT math involves percentage tiers and factors — binary floats are
  ruled out; integer cents get awkward with factor multiplication and VAT extraction.
- **Alternatives considered**: i64 cents (rounding gymnastics with factors), f64 (never for money).

### R8. Picker full-text search + usage ranking
- **Decision**: Postgres `pg_trgm` (GIN index) for fuzzy name search over drugs (via their
  packagings) and services; a nightly-refreshed usage-count table (`picker_usage`) counts
  Treatment Item references; results ordered by `usage DESC, similarity DESC`. One unified
  endpoint `GET /api/picker/items?q=` returns a discriminated union (drug packaging | service)
  so the UI renders distinct icons.
- **Rationale**: logic.md requires full-text search weighted by historical usage, refresh
  nightly is explicitly sufficient; pg_trgm handles typos without an external search engine.
- **Alternatives considered**: Postgres FTS/tsvector (word-stem oriented, worse for drug-name
  substrings), live-computed weights per query (needless per-keystroke aggregate).

### R9. GOT 2022 catalog import
- **Decision**: A one-time offline extraction of `requirements/GOT_2022.pdf` (Gebührenverzeichnis
  positions: number, description, single rate) into a generated, hand-reviewed SQL migration
  committed to the repo; all surgical positions (`Chirurgie` chapters of Teil C and other
  operative positions) inserted with `hidden = true`.
- **Rationale**: logic.md mandates import as SQL-inserts migration; hand review because PDF table
  extraction is imperfect and this is billing data.
- **Alternatives considered**: runtime PDF parsing (fragile, pointless for a one-time import).

### R10. Travel expenses (GOT § 10 Wegegeld) — verified from GOT 2022 text
- **Decision**: For a Treatment Item whose service has the travel-expense flag, the vet enters
  kilometers (one-way distance; the system bills per *Doppelkilometer*): price =
  `max(km × rate_per_double_km, minimum)` with config defaults `rate_per_double_km = 3.50 €`,
  `minimum = 13.00 €` (GOT 2022 § 10(2)); an optional multiplier ×1–×3 covers the
  "widrige Verkehrsverhältnisse" clause; proportional split across several customers on one
  trip is handled by the vet entering the pro-rated km. VAT applies like any line.
- **Rationale**: Verified against the GOT text in `requirements/GOT_2022.pdf` (§ 10:
  "je Doppelkilometer 3,50 Euro, insgesamt jedoch mindestens 13 Euro"). Rates in config
  because they change with GOT amendments.
- **Alternatives considered**: automatic distance from addresses (explicitly a future idea).

### R11. AMPreisV price calculation — structure verified from the official text
- **Decision** (amended: the column is `sales_price_net`, and the gross is derived from it — see
  requirements/prices.md): the sales price of an **original packaging** = list price net → vet may
  apply at most the pharmacy surcharges per **AMPreisV § 10**, which caps veterinarian
  surcharges at those of § 3(1) sentences 2–3, § 3(2)–(4), § 4(1)–(2), § 5(1)–(3), plus VAT.
  For a **subset packaging**: pro-rata net price of the dispensed quantity from the original
  packaging plus the § 4 Teilmengen surcharge, plus VAT. The exact tier tables/percentages are
  transcribed from https://www.gesetze-im-internet.de/ampreisv/ into `domain/money.rs` during
  implementation, pinned by unit tests with worked examples reviewed by the vet
  (constitution II; SC-005).
- **Rationale**: § 10's reference chain is confirmed; transcribing exact tier numbers from the
  primary source at implementation time (with the vet validating worked examples) avoids
  encoding a mis-summarized legal table into the design.
- **Alternatives considered**: fixed simple margin ("list price + VAT" mode) — deferred by
  decision in `requirements/prices.md`.

### R12. Frontend tooling alignment
- **Decision**: Replace the scaffold's `oxlint` with **biome** (lint + format) and add **knip**;
  TypeScript `strict`; full semver (exact versions) in `package.json`. Router: **TanStack
  Router** for typed route/search params (filters and current-record state live in the URL).
- **Rationale**: Constitution Principle I mandates biome + knip; typed search params directly
  serve the URL-state decision.
- **Alternatives considered**: react-router v7 (fine, but untyped search params make URL state
  stringly), keeping oxlint (contradicts constitution).

### R13. Session/config/deployment details
- **Decision**: Config via a single TOML file (path from `KVET_CONFIG` env var or CLI arg,
  default `/etc/k-vet/config.toml`), commented in de-DE and en-US, containing: user +
  argon2 password hash, invoice number pattern, currency, VAT rate choices, SMTP, base URL,
  optional Typst template path, invoice email template path, attachments dir, DB URL, listen
  address/port, log_level, Wegegeld rates. `figment` or `serde` + `toml` for parsing (no
  unwrap/expect).
- **Rationale**: Matches `requirements/global settings.md` config-vs-DB split exactly.
- **Alternatives considered**: env-only config (hostile to bilingual commented documentation).

### R14. Contact-data validation (email + phone)
- **Decision**: Email addresses validated in Rust (`email_address` crate or `validator`'s
  email check) and declared `format: email` in the OpenAPI spec so the generated zod client
  mirrors the rule. Phone: single optional field on Customer, validated with the
  `phonenumber` crate (Rust port of libphonenumber), default region `DE`, international input
  accepted; stored normalized as E.164, formatted nationally for display. Invalid input is
  never persisted (422 with field-level details); the auto-save flow keeps the last valid
  value while the field shows the error.
- **Rationale**: Validation lives once in Rust per the contract-generation decision
  (tech-decisions doc); parsing beats regexes for real-world German phone formats like
  "0171 / 123 45 67". Both validators are pure functions → money-math-style unit tests
  (Constitution II).
- **Alternatives considered**: lenient character-class regex (typos slip through, formats stay
  inconsistent), client-only validation (server must stay authoritative), multiple typed phone
  numbers (rejected via user decision — one optional number suffices).

### R15. Draft rows for auto-saved entities
- **Decision**: Form-edited entities (master data + appointments) are created immediately as
  `draft = true`; their mandatory columns are nullable in SQL and enforced by a completeness
  CHECK `draft OR (<mandatory columns> IS NOT NULL …)`. The server recomputes `draft` on every
  PATCH and flips it to `false` automatically with the write that completes the mandatory set
  — **no save button, the flag is never a user control**. The transition is one-way: clearing
  a mandatory field on a complete record is rejected (422, field-level error, last valid value
  kept — same rule as R14 validation). Drafts are excluded from pickers, treatment lines, and
  billing; lists show them as "incomplete — missing: …" and they are resumable. The nightly
  job (R5) deletes drafts older than 24 h with all completeness fields empty and no children.
  Kind-bound CHECKs filled during form entry (packaging supplier, service GOT fields) are
  draft-aware; discriminators chosen at creation (packaging kind, service type) stay NOT NULL.
  Details per table in data-model.md ("Draft rows" section).
- **Rationale**: Resolves the conflict between R4 (create-then-PATCH needs a row before the
  user has typed anything) and Constitution IV (completeness invariants must live in the DB):
  completed rows remain DB-guaranteed complete, while auto-save works from the first
  keystroke. Single storage model, no create-vs-edit UI split. The completeness CHECK is
  exercised by `#[sqlx::test]` integration tests (attempting `draft = false` with missing
  fields must fail).
- **Alternatives considered**: client-side draft buffer until minimally valid (pre-create
  input lives in one browser only — violates the auto-save guarantee; UI needs a create/edit
  mode split), empty-string/default placeholders under plain NOT NULL (the DB would bless
  incomplete records — guts Constitution IV; enums would need invented default values).

### R16. Logging
- **Decision**: `tracing` + `tracing-subscriber` with `EnvFilter` — level from the config file
  (`log_level`, default `info`), `RUST_LOG` env override for ad-hoc debugging. Compact
  human-readable output to stdout/stderr, captured by the systemd journal (journald) on the
  Pi — no log files, no rotation, no external log service. **Timestamps are human-readable in
  local time** (user requirement): tracing-subscriber's `ChronoLocal` timer (`chrono`
  feature), format `%Y-%m-%d %H:%M:%S%.3f` (e.g. `2026-07-26 14:32:05.123`), local = the Pi's
  system timezone. Request logging via tower-http `TraceLayer` (method, path, status,
  latency); errors logged with context where `AppError` is constructed. Complements the
  Prometheus `/metrics` endpoint.
- **Rationale**: journald already exists on the Pi (zero extra moving parts, Constitution V);
  EnvFilter is the ecosystem standard. `ChronoLocal` over the `time`-crate local-offset path
  because the latter is refused/unsound in multithreaded Linux processes, and over the UTC
  default because local time is what the vet correlates with real-world events. DB storage is
  unaffected (timestamptz per Constitution IV) — this is display-only.
- **Alternatives considered**: log files + rotation (needless ops surface next to journald),
  JSON structured logs (no aggregator exists to consume them; can be layered in later).

### R17. Invoice email templating
- **Decision**: **minijinja** renders the invoice email as **plain text UTF-8** from the
  operator-editable template file next to the config (a default template is embedded in the
  binary; the config file path overrides it). Template file layout: first line = subject
  template, blank line, body template. Template variables: salutation + customer name(s)
  (both names for two-name customers), invoice number, invoice date and amount formatted
  de-DE, practice name. Invoice emails are always de-DE (spec FR-030).
- **Umlauts / rendering correctness**: Rust strings are UTF-8 natively — minijinja passes
  ä/ö/ü/ß through losslessly. Client-side rendering is decided in the mail layer: lettre
  declares `charset=utf-8` with proper content-transfer-encoding for the body and RFC 2047
  encoded-words for non-ASCII **headers** (the subject line is where naive implementations
  break). A test asserts umlauts in subject and body survive template rendering and message
  building.
- **Rationale**: the template is a runtime-loaded user-editable file (per
  `requirements/global settings.md`), which rules out compile-time engines; minijinja is a
  zero-dependency single crate (Pi footprint, Constitution V) with Jinja2 conditionals for
  greeting variants ("Sehr geehrte Frau X", "… Frau X und Herr Y"). Plain text renders
  identically in every client — the attached PDF is the document that matters.
- **Alternatives considered**: Tera (same syntax, heavier dependency tree), handlebars-rust
  (logic-less; weak for conditional greetings), hand-rolled placeholder substitution (no
  conditionals — outgrown by the first greeting variant), Askama (compile-time; conflicts
  with the editable-file requirement), HTML multipart body (client-rendering variance and a
  second template to maintain, deferred).

### R18. Invoice number pattern
- **Decision**: The config pattern string supports the placeholders `{year}` (4-digit),
  `{month}` (2-digit, optional) and `{counter}` / `{counter:N}` (zero-padded to width N);
  literal text passes through. Example: `"{year}-{counter:4}"` → `2026-0042`;
  `"R-{year}/{counter:5}"` → `R-2026/00042`. The **scope is derived from the pattern's date
  parts** rendered for the invoice date: `{year}` → scope `"2026"` (counter resets yearly via
  a fresh `invoice_number_sequence` row), `{year}`+`{month}` → `"2026-03"`, no date parts →
  one global scope. Allocation is atomic:
  `INSERT … ON CONFLICT (scope) DO UPDATE SET counter = counter + 1 RETURNING counter` —
  monotonic, never decremented. The pattern is validated at startup (must contain
  `{counter}`, only known placeholders; invalid pattern = boot-time config error). If a
  pattern flip-flop ever re-renders a historical number, the `UNIQUE (invoice_number)`
  constraint rejects it and allocation increments and retries — collisions cannot persist.
  Date parts come from `invoice_date` (set at creation, re-stamped on update-recreate).
- **Rationale**: deriving the scope from the pattern's date parts answers "does the counter
  reset yearly?" without a second config knob; fail-fast validation surfaces config mistakes
  at boot instead of at invoicing time; the UNIQUE backstop makes collisions structurally
  impossible to persist.
- **Alternatives considered**: separate reset-interval setting (redundant with the pattern's
  own date parts), storing the pattern in the DB settings row (contradicts the config-vs-DB
  split — changing the pattern is a deliberate structural event per
  `requirements/global settings.md`).
