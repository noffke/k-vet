# Testing & CI

## Principles
- **Real Postgres everywhere, never a mock DB** — sqlx has no mock seam, and the data model deliberately pushes invariants into the DB (kind-enum CHECK constraints, partial unique indexes, composite FKs). Tests must exercise exactly those.
- **E2E runs against the full stack** (real binary + real Postgres), not a standalone frontend — API mocking stays at the component-test level.
- **Contract drift is a CI failure**: the OpenAPI spec and the generated TS client are committed; CI regenerates and fails on diff.

## Test layers
1. **Rust unit tests** (`cargo test`, no DB) — the money math: AMPreisV subset price formula, GOT factor / travel expenses, VAT, invoice number pattern.
2. **Backend integration tests — the workhorse.** Axum tested in-process (`tower::ServiceExt::oneshot`, or a spawned server + reqwest where streaming matters) on top of `#[sqlx::test]` databases. Covers handler → SQL → constraints in one test: stock-ledger semantics (FEFO split across lots, cancellation reversals, append-only after invoicing), price snapshots, CHECK/index enforcement.
3. **Frontend component tests** — Vitest + React Testing Library for the form-heavy screens; **MSW** for API mocking, kept honest by the OpenAPI-generated zod schemas and types.
4. **E2E — Playwright, full-stack.** Build the real binary, spawn it against a real Postgres, drive the UI. This is what catches auto-save actually persisting (top UX requirement), invoice PDF generation, and stock side effects. Two Playwright projects: desktop viewport and Pixel 9a viewport (responsive requirement).

## Database provisioning for tests
- **`#[sqlx::test]`**: per-test database, migrations applied automatically, parallel, auto-cleanup. No testcontainers — plain Postgres is simpler and faster.
- CI: Postgres as a GitHub Actions `services:` container.
- Local: `docker compose up db`.
- Commit the `.sqlx` offline query metadata so builds need no live DB; CI runs `cargo sqlx prepare --check` to catch drift.

## CI (GitHub Actions)
On PR / push, three jobs:
- **backend**: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo sqlx prepare --check`, `cargo test` (Postgres service container).
- **frontend**: biome, knip, `tsc --noEmit`, vitest, OpenAPI/codegen drift check.
- **e2e**: build the binary, spawn with Postgres service container, run Playwright (desktop + Pixel 9a projects).

On tag/release: cross-compile aarch64 with cargo-zigbuild, upload the binary as release artifact.

Caching: `Swatinem/rust-cache` + npm cache; Playwright browser cache keyed on the Playwright version.
