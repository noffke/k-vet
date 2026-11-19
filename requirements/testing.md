# Testing & CI

## Principles
- **Real Postgres everywhere, never a mock DB** — sqlx has no mock seam, and the data model deliberately pushes invariants into the DB (kind-enum CHECK constraints, partial unique indexes, composite FKs). Tests must exercise exactly those.
- **E2E runs against the full stack** (real binary + real Postgres), not a standalone frontend — API mocking stays at the component-test level.
- **Contract drift is a CI failure**: the OpenAPI spec and the generated TS client are committed; CI regenerates and fails on diff.

## Test layers
1. **Rust unit tests** (`cargo test`, no DB) — the money math: AMPreisV subset price formula, GOT factor / travel expenses, VAT, invoice number pattern.
2. **Backend integration tests — the workhorse.** Axum tested in-process (`tower::ServiceExt::oneshot`, or a spawned server + reqwest where streaming matters) on top of a real Postgres database per test. Covers handler → SQL → constraints in one test: stock-ledger semantics (FEFO split across lots, cancellation reversals, append-only after invoicing), price snapshots, CHECK/index enforcement.
3. **Frontend component tests** — Vitest + React Testing Library for the form-heavy screens; **MSW** for API mocking, kept honest by the OpenAPI-generated zod schemas and types.
4. **E2E — Playwright, full-stack.** Build the real binary, spawn it against a real Postgres, drive the UI. This is what catches auto-save actually persisting (top UX requirement), invoice PDF generation, and stock side effects. Two Playwright projects: desktop viewport and Pixel 9a viewport (responsive requirement).

## Database provisioning for tests
- **A database per test, cloned from a migrated template** (`k-vet-backend/tests/api/common/db.rs`, decided 2026-11-19): the migrations run once into a template named after their checksums, and each test gets `CREATE DATABASE … TEMPLATE …` — the same pristine, migrated and seeded state (the GOT catalogue among it), its own, so tests stay parallel. That replaced `#[sqlx::test]`, which ran all migrations into a fresh database for every test: about 1.5 s of setup per test against 0.2 s for a clone, and the backend run went from 258 s to 137 s. Truncating one shared database was considered and not taken: the migrations seed data the tests rely on, and one database would serialise the tests. A passed test's database is dropped; a failed one's is kept for inspection and swept by the next run. No testcontainers — plain Postgres is simpler and faster.
- **One integration-test binary** (`k-vet-backend/tests/api/`, a module per area): twenty-nine binaries each linked the whole application; one cut the clean build from 283 s to 171 s, the target directory from 31 GB to 8 GB, and — all tests now running in parallel rather than binary by binary — the run to 66 s.
- **E2E: a backend and a database per Playwright worker** (`e2e/fixtures/backend.ts`, `fixtures/test.ts`, decided 2026-11-19): each worker drops and creates `kvet_e2e_w<n>`, starts the real binary on it with its own port and config, and keeps both for every spec it runs — across files and both viewport projects; a worker replaced after a failure starts afresh. Files are spread across the workers (`fullyParallel` stays off, so a file's tests keep their order); `E2E_WORKERS` sets how many, else 4 locally and 2 in CI. One shared backend had kept seven of eight cores idle. With the test login's hash made cheap as well (the same `m=8,t=1,p=1` the backend tests use), the suite went from 796 s to 493 s (cheap hash) to 204 s with 4 workers (284 s with the 2 CI has).
- **E2E, measured after that** (4 workers, one change at a time): compression built at opt-level 3 in the dev profile — the debug binary took 314 ms to gzip the frontend bundle every test loads — 202 s → 176 s; traces of failures kept without the screencast, 176 s → 156 s (no tracing at all: 134 s); `signIn` through the API and straight to the test's first page instead of the form and the Übersicht, 156 s → 140 s. With the 2 workers CI has: 284 s → 207 s. Not taken: a release binary (202 s → 157 s, but 166 s to rebuild after a change against 7 s, and minutes more in CI); Postgres with `fsync`, `synchronous_commit` and `full_page_writes` off (202 s → 196 s, within the noise here); caching the seed helpers' API logins (5 ms each).
- CI: Postgres as a GitHub Actions `services:` container.
- Local: `docker compose -f docker-compose.dev.yml up db`.
- Commit the `.sqlx` offline query metadata so builds need no live DB; CI runs `cargo sqlx prepare --check` to catch drift.

## CI (GitHub Actions)
On PR / push, three jobs:
- **backend**: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo sqlx prepare --check`, `cargo test` (Postgres service container).
- **frontend**: biome, knip, `tsc --noEmit`, vitest, OpenAPI/codegen drift check.
- **e2e**: build the binary, spawn with Postgres service container, run Playwright (desktop + Pixel 9a projects).

On a `v*` tag: the same three jobs, plus **image** (the Dockerfile still builds) and **release** (publish the GitHub release with generated notes). Nothing is cross-compiled and no binary is uploaded — the image is built on the Pi from the tag.

Caching: `Swatinem/rust-cache` + npm cache; Playwright browser cache keyed on the Playwright version.
