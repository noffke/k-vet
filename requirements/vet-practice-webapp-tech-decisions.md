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
- **One container image**, assembled on `ubuntu:noble`: the backend binary plus the built frontend as files, API and static assets on one port.
- Multi-stage `Dockerfile` at the repo root: Node stage builds `k-vet-web`, Rust stage builds the backend (`SQLX_OFFLINE=true` against the committed `.sqlx/`), runtime stage copies both onto Ubuntu.
- **Built on the target machine** (the Pi, natively for aarch64) with `docker build`. No registry, no cross-compilation, no release artefacts to publish; CI only verifies on a `v*` tag that the image still builds, and publishes a GitHub release as the record of what that tag contains.
- **The image is tagged with its release version, never a moving tag.** `docker build --build-arg KVET_VERSION=1.2.3 -t k-vet:1.2.3 .` The version is the git tag; it is deliberately *not* taken from `Cargo.toml`, because that value is baked into the committed `openapi.json` and bumping it would make every release a codegen-drift commit.
- The frontend is *not* embedded in the binary: the backend serves it from `server.web_dir`, which the image points at `/usr/share/k-vet/web`.
- Runtime state is host-mounted: `config.toml`, the templates directory and the attachments directory. The container runs as the image's `ubuntu` user (uid/gid 1000), so those paths must be accessible to that uid/gid; the entrypoint checks this and seeds missing templates.
- **rustls** everywhere (not openssl) — keeps the build free of system TLS libraries; the invoice PDF fonts are compiled in, so the runtime image needs no font packages.
- **PostgreSQL is the Pi's own server**, not a container: the appliance already runs one. Containers reach it over the docker bridge (`--add-host=host.docker.internal:host-gateway`), which needs a `listen_addresses` and a `pg_hba.conf` entry — see `docs/installation.md`.
- **A systemd template unit, `k-vet@.service`, instead of a Compose service** — because there is more than one instance. Hooks into the existing Prometheus monitoring via `/metrics`, health via `/healthz`.

### Two environments

Production and staging run on the same Pi, from one image and one Postgres server, as
`k-vet@prod` and `k-vet@staging`. Each instance owns its `config.toml`, templates directory,
**attachments directory** and database; `KVET_VERSION` in `/etc/k-vet/<instance>.env` is the
only thing that decides which image it runs, so production never moves because something else
was built.

This is a deliberate exception to Constitution V's YAGNI clause, and the justification is
narrow: the practice bills from this application, and migrations are forward-only. There is no
undo for a schema change that turns out wrong, so there has to be somewhere to meet it first.
The cost is bounded — one extra systemd instance and one extra database, no new runtime
services, and the image, the build and the artefact are unchanged.

Separate attachments directories are a **correctness** requirement rather than tidiness:
attachment storage is content-addressed and deduplicated, and `jobs::sweep_orphan_attachments`
decides what to unlink by querying its own database alone. Two instances sharing a directory
would let one delete blobs the other still references.
