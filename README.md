# k-vet

Practice management for a one-vet veterinary practice: appointments, patients, treatments,
pharmacy stock, German fee-schedule billing (GOT 2022 / AMPreisV) and the monthly hand-off to
the bookkeeper.

The practice runs the app on a Raspberry Pi in the office. That shapes the technical decisions
here: one binary, one database, no cloud services, no build step at run time. The vet works on a
laptop in the practice and on a Pixel 9a during house calls, so both layouts are
first-class — not one design squeezed onto a phone.

## What it does

- **Appointments and treatments** — an appointment holds one or more treatments; a treatment
  carries the reason, the finding and the billed lines, in the order the vet wants them.
- **Pharmacy** — drugs with original and subset packagings, lot-based stock with FEFO
  dispensing, stocktakes that book the difference, and an append-only movement history.
- **Billing** — GOT 2022 positions with factors and Wegegeld (§ 10), drug prices from AMPreisV
  § 3, § 4 and § 10, VAT summarised per rate, invoice numbers from an operator-defined pattern
  with a counter that never goes backwards.
- **Invoices** — PDF from a Typst template, plain-text email through the practice's own mail
  server, and a lifecycle (created → accepted → submitted, or cancelled) where every step is
  stamped.
- **Bookkeeping hand-off** — every pending PDF as one ZIP, then one click to mark them handed
  over.
- **Everything auto-saves.** A record is created as a draft on the first keystroke and completes
  itself once the required fields are there; nothing is lost to a missing save button.

## Architecture in one screen

```
┌──────────────────────────── k-vet-backend (Rust) ────────────────────────────┐
│ axum 0.8 ── tower-sessions (Postgres store) ── argon2 single-user login      │
│   src/api/        one module per resource, utoipa-annotated → openapi.json   │
│   src/domain/     money (AMPreisV, GOT), stock (FEFO), invoice numbers, files│
│   src/pdf/        Typst template → invoice PDF                              │
│   src/mail/       minijinja template → plain-text email (lettre)            │
│   src/jobs/       nightly picker weights, draft cleanup, orphan sweep        │
│   migrations/     the invariants live here: CHECKs, partial unique indexes   │
│   static_assets   rust-embed of k-vet-web/dist (feature `embed-frontend`)    │
└──────────────────────────────────────────────────────────────────────────────┘
        │ sqlx 0.9, compile-time-checked SQL                    ▲ /api, /metrics
        ▼                                                       │
   PostgreSQL 16                                    k-vet-web (React 19 + Vite)
                                                    TanStack Router + Query,
                                                    orval-generated client,
                                                    Tailwind 4, de-DE default
```

The client contract is generated, never hand-written: handlers → `openapi.json` → orval →
`k-vet-web/src/api/generated/`. Both outputs are committed and CI fails on drift.

## Development setup

Requirements: Rust stable, `sqlx-cli`, Node.js `^20.19 || >=22.12`, Docker (for Postgres) and
`poppler-utils` (the E2E suite reads an invoice PDF with `pdftotext`).

```bash
docker compose up -d db                            # Postgres on localhost:5432

cd k-vet-backend
cp ../config.example.toml config.toml              # bilingual comments; set user + password
cargo run -- --hash-password                       # argon2id hash for the config file
cargo sqlx migrate run                             # includes the GOT 2022 import
cargo run                                          # API on :8080

cd ../k-vet-web
npm ci && npm run dev                              # UI on :5173, proxies /api to :8080
```

## Commands

```bash
# Backend: format, lint, SQL metadata, unit + integration tests (against real Postgres)
cd k-vet-backend
cargo fmt --check && cargo clippy --all-targets -- -D warnings \
  && cargo sqlx prepare --check -- --all-targets && cargo test

# Frontend: lint/format, dead code, types, component tests
cd k-vet-web
npx biome check && npx knip && npx tsc --noEmit && npm test

# End to end: the real binary with the embedded frontend, desktop + Pixel 9a
cd k-vet-web && npm run build
cd ../k-vet-backend && cargo build --features embed-frontend
cd ../e2e && npx playwright test
```

## Regenerating the contract

Run this after any change to a handler signature, request body or response type:

```bash
cd k-vet-backend
cargo run --quiet --bin export-openapi > openapi.json
cargo sqlx prepare -- --all-targets                # .sqlx/ offline metadata
cd ../k-vet-web && npm run generate:api            # src/api/generated/
```

## Release

Tagging `v*` cross-compiles for the Pi (aarch64) with the frontend embedded:

```bash
cd k-vet-web && npm run build
cd ../k-vet-backend
cargo zigbuild --release --features embed-frontend --target aarch64-unknown-linux-gnu
```

## Documentation

- [docs/installation.md](docs/installation.md) — installing and running it on a Linux host
- [docs/templates.md](docs/templates.md) — the invoice PDF and invoice email templates
- [specs/001-vet-practice-app/](specs/001-vet-practice-app/) — specification, plan, data model,
  research and the task list this implementation followed
- [.specify/memory/constitution.md](.specify/memory/constitution.md) — the rules the code holds
  itself to: tests are mandatory, no `unwrap` in library code, both viewports, generated
  contracts, invariants in the database

## Licence

Private project; all rights reserved.
