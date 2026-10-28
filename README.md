# WORK IN PROGRESS!

# k-vet

Practice management for a one-vet veterinary practice: appointments, patients, treatments,
pharmacy stock, German fee-schedule billing (GOT 2022 / AMPreisV) and the monthly hand-off to
the bookkeeper.

The practice runs the app on a Raspberry Pi in the office. That shapes the technical decisions
here: one container image, one database, no cloud services, no registry, no build step at run
time. The vet works on a laptop in the practice and on a Pixel 9a during house calls, so both
layouts are first-class — not one design squeezed onto a phone.

## What it does

- **Appointments and treatments** — an appointment holds one or more treatments; each animal's
  record carries the Vorbericht, the Therapie and the billed lines, in the order the vet wants
  them. Both texts take bold, italic, underline and lists, with typography as you type
  (`->` becomes `→`), and the invoice prints them that way. **Textbausteine** are a library of
  named snippets, formatted too, that drop into either text at the cursor, so the sentences that
  repeat every visit are not retyped on a phone. The list splits into **Anstehend** (by day) and
  **Vergangen**.
- **Mitbringen and Packliste** — drugs a customer phoned in for go on the future appointment as
  reminders (nothing dispensed yet); each day's Packliste adds them up, checks the shelf, and is
  ticked off while packing. At the visit one click makes a reminder a dispensed position.
- **Calendar and contacts on the phone** (optional) — k-vet serves the appointments over CalDAV
  and the customers over CardDAV to a sync app on the vet's phone, through the practice's VPN:
  customer, animals, address, phone, note and the drugs to bring in each entry; each customer a
  contact named with the animals ("Erika Mustermann (Bello, Minka)"), so a call shows who it is.
  Entries and contacts made on the phone come into k-vet; nothing goes to a third party.
- **Pharmacy** — drugs with original and subset packagings, lot-based stock with FEFO
  dispensing, stocktakes that book the difference, and an append-only movement history.
- **Billing** — GOT 2022 positions with factors and Wegegeld (§ 10), drug prices from AMPreisV
  § 3, § 4 and § 10, VAT summarised per rate, invoice numbers from an operator-defined pattern
  with a counter that never goes backwards. A Wegegeld line fills in its kilometres — the
  customer's last visit, else Google Maps' shortest route — and no invoice goes out without them.
- **Google Maps** (optional key) — a customer's address is checked as it is typed, with a
  misspelling offered corrected; the customer and the appointment open the drive there in
  Google Maps.
- **Invoices** — PDF from a Typst template, plain-text email through the practice's own mail
  server, and a lifecycle (created → accepted → sent → submitted, or cancelled) where every step
  is stamped. Mail goes out through a queue — now, or at a chosen hour — retried for six hours when
  the mail server does not answer. An invoice reaches the customer — by email or on paper — before the bookkeeper sees it,
  and the dashboard lists the ones that did not and every mail that failed.
- **Preisauskunft** — for a caller asking what a treatment would cost: positions, groups and
  Wegegeld (for a customer or a typed address) priced exactly as an invoice would, with the gross
  total to tell. Kept on the customer's page and taken into their visit at the prices quoted,
  with any change since shown.
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
│   static_assets   the built frontend from `server.web_dir`, SPA fallback     │
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

Requirements: Rust stable, `sqlx-cli`, Node.js 24 LTS (22.19 at least), Docker (for Postgres) and
`poppler-utils` (the E2E suite reads an invoice PDF with `pdftotext`).

```bash
docker compose -f docker-compose.dev.yml up -d db  # Postgres on localhost:5432

cd k-vet-backend
cp ../config.example.toml config.toml              # bilingual comments; set user + password,
                                                   # and attachments_dir to a writable path
cargo run -- --hash-password                       # argon2id hash for the config file
KVET_TEMPLATES_DIR=templates cargo run             # API on :8080, migrations run at start;
                                                   # email templates from k-vet-backend/templates

cd ../k-vet-web
npm ci && npm run dev                              # UI on :5173, proxies /api to :8080
```

To run the application as its image instead, without Rust or Node on the machine:

```bash
cp config.example.toml config.toml                 # listen_address = "0.0.0.0",
                                                   # database url host `db` instead of localhost
docker compose -f docker-compose.dev.yml up -d --build   # localhost:8080, tierarzt / changeme
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

# End to end: the real binary serving the built frontend, desktop + Pixel 9a
cd k-vet-web && npm run build
cd ../k-vet-backend && cargo build
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

The deployment artefact is a container image, built on the machine that runs it — no registry is
involved. `Dockerfile` has three stages (Node builds the frontend, Rust builds the backend, the
result is assembled on `ubuntu:noble`), and a systemd template unit runs it against the host's
own PostgreSQL, with the configuration, the templates and the uploaded files mounted from the
host.

A release is a pushed `v*` tag: CI re-runs every gate on it, proves the image builds, and
publishes the release notes. The Pi then builds that tag once and runs it — first on the staging
instance, then, unchanged, on the practice's own:

```bash
scripts/cut-release.sh              # here — next patch; pass 1.3.0 for a bigger bump
deploy/promote.sh staging 1.2.3     # on the Pi
deploy/promote.sh prod    1.2.3     # the same image, once it has been tried
```

The container runs as uid/gid 1000, so those three mounted paths must be accessible to that
uid/gid. Full walkthrough in [docs/installation.md](docs/installation.md), the release process in
[docs/releasing.md](docs/releasing.md).

## Documentation

- [docs/installation.md](docs/installation.md) — building the image and running it on a Pi
- [docs/templates.md](docs/templates.md) — the invoice PDF and invoice email templates
- [specs/001-vet-practice-app/](specs/001-vet-practice-app/) — specification, plan, data model,
  research and the task list this implementation followed
- [.specify/memory/constitution.md](.specify/memory/constitution.md) — the rules the code holds
  itself to: tests are mandatory, no `unwrap` in library code, both viewports, generated
  contracts, invariants in the database

## Licence

[GNU General Public License v3](LICENSE).

### Third-party components

The invoice PDF sets in **Source Sans 3**, © 2010–2024 Adobe, under the
[SIL Open Font License 1.1](k-vet-backend/fonts/LICENSE.txt). The four faces in
`k-vet-backend/fonts/` are compiled into the binary (`src/pdf`), because the appliance has no
font packages installed.

They stay under the OFL rather than the GPL, and that is not a conflict: a font is data, not
linked code, so it is neither a derivative of the program nor the program a derivative of it.
The OFL expressly allows bundling with software under any licence (clause 2) and asks only that
the notice accompany every copy — which is why the deployment image carries it at
`/usr/share/k-vet/licences/`. The faces are shipped unmodified; "Source" is a Reserved Font
Name, so a modified version may not keep it (clause 3).
