# Vet Practice Webapp — Tech Stack TL;DR

Single-user, records/billing-centric practice management app, self-hosted on a Raspberry Pi. Appointment scheduling stays in Google Calendar (out of scope).

## Backend
- **Rust: Axum + sqlx** (Tokio stack; compile-time-checked SQL). SeaORM is the fallback if a more JPA-like entity layer is wanted.
- Chosen over Kotlin/Spring mainly for deployment shape (single static binary, ~15 MB RSS, no JVM on the Pi), not raw necessity.

## Database
- **PostgreSQL**, running on a **USB SSD — never the SD card** (WAL write patterns kill SD cards).
- SQLite was considered viable for single-user, but Postgres wins on ad-hoc query tooling for record data.

## API contract / type safety
- **utoipa** on Axum handlers → OpenAPI spec.
- Generate from the spec: TypeScript types + **TanStack Query hooks** (`orval` or `openapi-react-query`) and **zod schemas** (`openapi-zod-client`) so validation rules live once, in Rust.
- Alternative for simple cases: `ts-rs` straight from Rust structs.

## Frontend
- **Vite + React**, **shadcn/ui + Tailwind**.
- **TanStack Table** (headless) for lists — render as a table on desktop, collapse to stacked cards on mobile (Pixel 9a). This responsive split is why headless beats packaged DataTables (Mantine/PrimeVue rejected for this reason).
- **react-hook-form + zod** for the form-heavy screens (visits, treatments, billing line items).
- **Async combobox** (shadcn/cmdk, debounced) for owner/animal search — the highest-frequency interaction in the app.
- Dioxus/Leptos rejected: ecosystem gap on tables/pickers/forms would eat the project time.

## State management
- **TanStack Query** for all server state (cache, background refetch, optimistic updates).
- Filters/search/current record → **URL state** (router params).
- Forms → react-hook-form. Odd toggles → `useState`.
- **No Zustand** unless genuinely global client-only state emerges later (e.g. cross-page invoice draft, offline queue).

## Binary attachments (photos, lab PDFs)
- **Filesystem, not bytea** — avoids WAL amplification, keeps pg_dump small, enables streaming via tower-http `ServeDir` and cheap incremental backups.
- **Content-addressed storage**: files at `attachments/ab/cd/<sha256>`, metadata table in Postgres (`visit_id`, `sha256`, `mime_type`, `size_bytes`, `orig_name`, `created_at`).
- Upload: stream to temp file while hashing → atomic rename → insert row. Monthly orphan-file sweep. Dedup for free.
- Generate thumbnails at upload time (`image` crate), also content-addressed. Serve with `Cache-Control: immutable`.
- Attachments dir lives on the same USB SSD as Postgres.

## Invoices / PDF generation
- **Typst as an embedded Rust library** — typeset invoices from a `.typ` template with structured data. No headless Chrome, no external tools on the Pi.

## Backups
- **Extend the existing borgmatic setup** (restic evaluated, no advantage here).
- Use borgmatic's `postgresql_databases:` hook (with `format: custom` for selective/parallel `pg_restore`) + the attachments directory in the same archive → consistent snapshot.
- Wire borgmatic's `healthchecks:` option into the existing Healthchecks.io dead-man's-switch.
- Do one full test restore (DB + attachments, open a photo) before trusting it.

## Testing & CI
- **GitHub Actions**; real Postgres via `#[sqlx::test]` (no mocks, no testcontainers); **Playwright full-stack E2E** against the real binary. Details in `testing.md`.

## Deployment
- One executable: built frontend embedded via **rust-embed**, API + static assets on one port.
- Cross-compile with **cargo-zigbuild** for aarch64, use **rustls** (not openssl) to keep cross-builds painless.
- systemd unit on the Pi; hooks into the existing Prometheus monitoring.
