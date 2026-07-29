# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Practice management for a **single-vet German veterinary practice**, deployed as **one binary
plus PostgreSQL** on a Raspberry Pi. Single user, no multi-tenancy, no runtime services beyond
Postgres. The vet works on a laptop *and* on a Pixel 9a during house calls — both viewports are
first-class.

Three documents outrank your judgement and each other in this order:

1. `.specify/memory/constitution.md` — hard rules (no `unwrap`/`expect` in Rust library code,
   tests mandatory, auto-save everywhere, responsive both viewports, invariants in the database,
   de-DE default with locale-aware formatting *and* parsing).
2. `requirements/*.md` — domain truth (`logic.md`, `prices.md`, `datamodel.md`,
   `global settings.md`, `testing.md`, `vet-practice-webapp-tech-decisions.md`). A conflict with
   the constitution is resolved by amending one of the two, never by ignoring it.
3. `specs/001-vet-practice-app/` — the spec, plan, data model, research and the task list this
   implementation followed (all 85 tasks complete). `spec.md` FR-xxx / SC-xxx numbers are cited
   in code comments and tests; keep citing them.

`README.md`, `docs/installation.md` and `docs/templates.md` are the user-facing docs — update
them when behaviour they describe changes.

## Commands

Postgres for development and tests:

```bash
docker compose up -d db      # postgres://kvet:kvet@localhost:5432/kvet
export DATABASE_URL=postgres://kvet:kvet@localhost:5432/kvet
docker compose exec db createdb -U kvet kvet_e2e   # once; the E2E suite wants its own database
```

Backend (`k-vet-backend/`), the same gates CI runs:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo sqlx prepare --check -- --all-targets     # committed .sqlx/ must match the queries
cargo test                                      # unit + integration, real Postgres
cargo run                                       # API on :8080 (KVET_CONFIG or ./config.toml)
cargo run -- --hash-password                    # argon2id hash for [auth] password_hash
cargo run -- --reset-database                   # needs KVET_ALLOW_DB_RESET=1

cargo test --test invoices                                  # one integration test binary
cargo test --test invoices creating_an_invoice_allocates     # one test by name substring
cargo test --lib money::                                    # unit tests of one module
```

Frontend (`k-vet-web/`):

```bash
npm run dev            # :5173, proxies /api to :8080
npx tsc --noEmit && npx biome check && npx knip && npm test
npx biome check --write src                       # format + autofix
npx vitest run tests/format.test.ts -t "en-US"    # one test file / one case
```

End to end (`e2e/`) — runs the **real binary** against a real database:

```bash
cd k-vet-web && npm run build                        # dist/ is embedded, so build it first
cd ../k-vet-backend && cargo build --features embed-frontend
cd ../e2e && DATABASE_URL=postgres://kvet:kvet@localhost:5432/kvet_e2e npx playwright test
npx playwright test core-loop --project=desktop      # one spec, one viewport
KVET_BINARY=../k-vet-backend/target/release/k-vet-backend npx playwright test   # release check
```

A plain `cargo build` overwrites `target/debug/k-vet-backend` **without** the frontend; rebuild
with `--features embed-frontend` before any browser check, or the page just says the frontend is
not embedded.

## The contract pipeline (never edit generated files)

`src/api/*` handlers with `#[utoipa::path]` → `cargo run --bin export-openapi > openapi.json` →
`npm run generate:api` (orval) → `k-vet-web/src/api/generated/`. Both artefacts are committed and
`scripts/check-codegen-drift.sh` fails CI on any diff. After touching a handler signature,
request body or response type:

```bash
cd k-vet-backend && cargo run --quiet --bin export-openapi > openapi.json \
  && cargo sqlx prepare -- --all-targets
cd ../k-vet-web && npm run generate:api
```

A new endpoint needs three registrations: the route in `src/api/<module>.rs::routes()`, the
module merged in `src/api/mod.rs`, and the handler listed in the `paths(...)` block in
`src/lib.rs`. Every `#[utoipa::path]` **must** carry an explicit `operation_id` — orval derives
hook names from it and silently collides otherwise.

## Backend architecture

`src/main.rs` is a thin wrapper; the whole application lives in `src/lib.rs::build_app()` so
integration tests drive the real router in-process. Layers, outermost first: request body limit →
session (`tower-sessions` with a hand-written `PostgresSessionStore` — the upstream store pins
sqlx 0.8) → metrics → tracing. `/api/*` is behind `auth::require_auth`; `/healthz` and `/metrics`
are open; everything else falls through to `static_assets::handler` (rust-embed + SPA fallback).

- `src/domain/money.rs` — the money law: AMPreisV § 3/§ 4/§ 10 drug pricing bands, GOT § 10
  Wegegeld `max(km × rate, minimum) × multiplier(1–3)`, VAT per rate, rounding. Pure functions,
  worked-example tests carrying `⚠ FOR VET REVIEW` comments. **Do not compute money anywhere
  else** — templates and the frontend only place pre-formatted strings.
- `src/domain/stock.rs` — FEFO allocation and the append-only movement ledger. A dispense is a
  *draft* until the invoice is accepted, then *frozen*; corrections are compensating rows that
  reference what they reverse. A movement with a reversal is never deleted. Line quantity counts
  **packagings**; the deduction is `quantity × packaging.quantity` base units taken from the
  drug's **original** packaging lots (`stock::stock_target`).
- `src/api/invoices.rs` — lifecycle `created → accepted → submitted`, or `cancelled`. Updating a
  created invoice keeps its number; updating an accepted one cancels it and burns the number
  (`domain/invoice_number.rs` allocates from the configured pattern, counter scoped by the
  pattern's date parts, never reused). Every step is timestamped.
- `src/pdf/` (Typst) and `src/mail/` (minijinja + lettre) — see `docs/templates.md`. Only the
  fonts embedded in the binary are available; the email template's first line is the subject.
- `src/jobs/mod.rs` — in-process scheduler: nightly at 03:00 rebuild picker weights and delete
  abandoned drafts, monthly sweep of unreferenced attachments. Each job is a plain async
  function so tests call it directly.

### Database and sqlx

Migrations in `k-vet-backend/migrations/` are the specification of the invariants: CHECK
constraints (`draft OR (…)` completeness sets), partial unique indexes, composite FKs, deferrable
position uniques, the `lot_remaining` view. Push new invariants down there and test them through
the API rather than guarding only in Rust.

sqlx 0.9 specifics that will bite you:

- Query macros are compile-time checked against `DATABASE_URL`; the committed `.sqlx/` metadata
  must be refreshed with `cargo sqlx prepare -- --all-targets` (CI runs with `SQLX_OFFLINE=true`).
- Dynamic SQL strings are rejected. Static SQL per case; in tests wrap deliberate interpolation
  in `sqlx::AssertSqlSafe`.
- Override inferred nullability with `AS "col!"` / `AS "col?"` — `AS "col: Option<T>"` yields
  `Option<Option<T>>`, and views make every column nullable.
- `FOR UPDATE` cannot touch the nullable side of an outer join; read that column separately.

### Draft rows and auto-save

There is no save button anywhere. `POST /api/<resource>` creates a `draft` row immediately so
field-level `PATCH` has something to write to; responses carry `draft` and `missing_fields`, and a
row completes itself (one-way) once its completeness set is satisfied. `PATCH` bodies use
`api::common::double_option` to distinguish "field absent" from "explicitly null".

Rust library code must not `unwrap`, `expect` or `panic` (denied in `Cargo.toml [lints.clippy]`);
tests may, via `clippy.toml` plus a per-file `#![allow(...)]`. `rustfmt.toml` is only `edition` +
`max_width` — run `cargo fmt`, never hand-format.

## Frontend architecture

Code-based TanStack Router (`src/router.tsx`): routes hang off an `app` layout route whose
`beforeLoad` resolves the session once and redirects to `/login`. TanStack Query is the only
server-state store — no Redux, no context for server data. Adding a page means a route in
`router.tsx` and an entry in `NAV` in `components/AppLayout.tsx` (desktop rail, phone tab bar,
overflow sheet).

- `lib/autosave.ts` — `useAutoSave` debounces 600 ms and flushes on blur, `visibilitychange`,
  `pagehide` and unmount; it surfaces per-field server errors. Use it for every editable record.
- `components/DataList.tsx` — one column definition renders a real table on desktop and cards on
  a phone. The card *is* a `<button>`, so per-row buttons/links go through `rowActions`, never
  into a `cell`. Row clicks ignore events from interactive elements.
- `lib/format.ts` + `useLocaleFormat` — all user-facing numbers, dates and money go through
  these, in both directions (de-DE `1.234,56 €` / `dd.MM.yyyy`, en-US equivalents). The wire
  format is always dot-decimal strings via `toWire`; never `toFixed`/`toLocaleString` in a
  component.
- i18n: no hardcoded user-facing strings — `src/i18n/de.json` and `en.json` must stay key-for-key
  identical (248 keys today), de-DE is the default.
- Visual identity is extracted from the practice's website into CSS custom properties in
  `src/index.css` (paper/cream/ink/rust/sage/blush/line, `.numeric` tabular figures, `.eyebrow`).
  Use the tokens, not raw hex. Touch targets stay ≥ 44 px on phones.

## Testing conventions

- Integration tests (`k-vet-backend/tests/*.rs`) use `#[sqlx::test]`, which gives each test its
  own migrated database, and drive the real router through `tests/common/mod.rs::TestApp`
  (`get`/`post`/`patch`/`post_multipart`, cookie jar, `seed_*` helpers). Prefer these over
  mocking anything.
- Playwright specs share **one** database across specs and both viewport projects. Fixtures must
  therefore generate unique names (`fixtures/seed.ts` suffixes with a timestamp) and assertions
  must not assume global counts — `clearPendingInvoices()` is how a spec establishes a known
  slate. Scope text assertions with `.filter({ visible: true })`, because both DataList layouts
  are in the DOM.
- `tests/helpers.ts::navigate()` clicks the right chrome for the viewport and asserts the page
  does not scroll sideways; horizontal overflow on a phone also shrinks Chrome's page scale and
  makes fixed-tab-bar clicks miss, so keep that assertion.
- The GOT 2022 catalogue (931 services, surgical positions hidden) is imported by migration
  `0008`, so every test database has it — pick fixtures that cannot collide with it.
- `docs`/spec claims are verified, not assumed: the settings E2E reads the generated invoice PDF
  back with `pdftotext` (needs `poppler-utils`).

## Release

`cargo zigbuild --release --features embed-frontend --target aarch64-unknown-linux-gnu` after
`npm run build`; CI does this on a `v*` tag. Practice name, address, IBAN, VAT ID, logo and
CC/BCC live in the database (settings page); mail server, invoice number pattern, currency, VAT
choices and template paths are operator configuration in `config.toml`
(`config.example.toml` is commented in both languages — keep it that way).
