# Implementation Plan: Vet Practice Management Application

**Branch**: `001-vet-practice-app` | **Date**: 2026-07-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-vet-practice-app/spec.md`

## Summary

Single-user veterinary practice management app — treatment documentation, pharmacy with
lot/batch stock bookkeeping, GOT/self-defined service billing, invoice lifecycle with PDF +
email, dashboard and settings — self-hosted on a Raspberry Pi. Rust (Axum + sqlx) backend with
PostgreSQL, Vite + React (shadcn/ui + Tailwind) frontend, OpenAPI-generated typed client,
Typst-rendered invoice PDFs, everything deployed as one static binary with the frontend
embedded. Stack per `requirements/vet-practice-webapp-tech-decisions.md`; test strategy per
`requirements/testing.md`; open pricing/travel/BTM questions resolved in
[research.md](research.md).

## Technical Context

**Language/Version**: Rust stable (edition 2024); TypeScript 5.x+ `strict`

**Primary Dependencies**: Backend: axum, sqlx (postgres, rustls), tokio, utoipa, tower-http,
tower-sessions (postgres store), typst, rust-embed, lettre (rustls), image, rust_decimal,
argon2, serde/toml, tracing + tracing-subscriber (EnvFilter, ChronoLocal local-time
timestamps — research R16). Frontend: react 19, vite, tailwind + shadcn/ui, @tanstack/react-query,
@tanstack/react-table, @tanstack/react-router, react-hook-form, zod, i18next/react-i18next,
cmdk; codegen: orval. Tooling: biome, knip, vitest, @testing-library/react, msw, playwright.

**Storage**: PostgreSQL (invariants in CHECK constraints / partial unique indexes / composite
FKs — see [data-model.md](data-model.md)); binary attachments content-addressed on the
filesystem (`attachments/ab/cd/<sha256>`), same USB SSD as Postgres

**Testing**: cargo test (unit money-math + `#[sqlx::test]` integration against real Postgres);
vitest + RTL + MSW (component); Playwright full-stack E2E, desktop + Pixel 9a projects; CI per
`requirements/testing.md`

**Target Platform**: Linux aarch64 (Raspberry Pi, systemd, existing Prometheus + borgmatic);
dev on x86_64 Linux; cross-compile via cargo-zigbuild, rustls only

**Project Type**: Web application — Rust API backend (`k-vet-backend/`) + React SPA
(`k-vet-web/`), shipped as one binary (frontend embedded via rust-embed)

**Performance Goals**: Single user; picker search results < 1 s (SC-003); backend idle
footprint in the ~15 MB RSS class; PDF generation interactive (< 2 s per invoice)

**Constraints**: No runtime services beyond PostgreSQL (in-process scheduled jobs); auto-save
everywhere (no explicit save); de-DE default UI + en-US; no unwrap/expect; DB timestamps
timestamptz; sequence PKs per entity; money as NUMERIC/rust_decimal

**Scale/Scope**: One practice, one user; low thousands of treatments/invoices per year;
~16 entities, ~7 top-level screens (dashboard, appointments/treatments, customers, patients,
pharmacy, services/templates, invoices, settings)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Evidence |
|---|---|---|---|
| I | Code Quality & Tooling Discipline | ✅ PASS | No unwrap/expect (thiserror/anyhow + `?`); clippy `-D warnings` + rustfmt in CI; TS `strict`; scaffold's oxlint replaced by biome + knip (research R12); full semver in Cargo.toml/package.json |
| II | Test-Backed Changes (NON-NEGOTIABLE) | ✅ PASS | Test pyramid designed in per `requirements/testing.md`; money math unit-tested against official worked examples (R10, R11); `#[sqlx::test]` exercises the CHECK/index invariants of data-model.md; Playwright desktop + Pixel 9a; OpenAPI + generated client committed, drift = CI failure |
| III | User Experience Consistency | ✅ PASS | Auto-save design R4 (debounced PATCH, immediate entity creation, E2E-verified); responsive via headless TanStack Table (table ⇄ cards); i18n de-DE default via react-i18next (R3); practice website palette mapped to Tailwind theme tokens; usability > aesthetics |
| IV | Data Integrity at the Database | ✅ PASS | Identity PKs per entity; timestamptz everywhere; kind-enum CHECKs, partial unique indexes, composite FK lot→original packaging; stock strictly derived (`lot_remaining` view); append-only movements after invoice acceptance |
| V | Simplicity & Single-User Scope | ✅ PASS | One binary + Postgres only; sessions in Postgres (no Redis); jobs in-process (R5); no Zustand; no multi-user constructs; deviations from recorded stack decisions require updating the tech-decisions doc first |

**Post-Phase-1 re-check**: ✅ PASS — the design introduced no new projects, services, or
patterns beyond the recorded decisions; no Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/001-vet-practice-app/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0: consolidated + new decisions (R1–R13)
├── data-model.md        # Phase 1: schema, constraints, state machines
├── quickstart.md        # Phase 1: setup, test commands, validation scenarios
├── contracts/
│   └── rest-api.md      # Phase 1: endpoint inventory (binding artifact: generated openapi.json)
└── tasks.md             # Phase 2 (/speckit-tasks — NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
k-vet-backend/
├── Cargo.toml               # full semver pins
├── config.example.toml      # bilingual comments (de-DE + en-US)
├── openapi.json             # committed utoipa output (CI drift check)
├── migrations/              # sqlx migrations incl. one-time GOT 2022 import
├── .sqlx/                   # committed offline query metadata
├── src/
│   ├── main.rs              # router, state, embedded assets, job spawn
│   ├── config.rs            # TOML config (R13)
│   ├── error.rs             # AppError → problem+json (no unwrap/expect)
│   ├── auth.rs              # login/logout, tower-sessions, argon2 (R2)
│   ├── api/                 # one module per resource (utoipa-annotated handlers)
│   │   ├── customers.rs, patients.rs, suppliers.rs, manufacturers.rs
│   │   ├── drugs.rs, packagings.rs, lots.rs, services.rs, templates.rs
│   │   ├── appointments.rs, treatments.rs, treatment_items.rs, invoices.rs
│   │   ├── attachments.rs, picker.rs, dashboard.rs, settings.rs
│   ├── domain/
│   │   ├── money.rs         # AMPreisV (R11), Wegegeld (R10), VAT — pure, unit-tested
│   │   ├── invoice_number.rs# pattern + sequence allocation
│   │   ├── stock.rs         # FEFO suggestion/split, draft recalcs, cancellation reversals
│   │   ├── draft.rs         # draft-row completeness recompute (R15)
│   │   ├── contact.rs       # email/phone validation + normalization (R14)
│   │   └── files.rs         # content-addressed attachment storage
│   ├── pdf/                 # typst invoice rendering (embedded template + override)
│   ├── mail/                # lettre SMTP (R6)
│   ├── jobs/                # nightly picker_usage refresh, monthly orphan sweep (R5)
│   └── bin/export-openapi.rs
└── tests/                   # integration: handler→SQL→constraint per area

k-vet-web/
├── package.json             # biome + knip + vitest; full semver pins
├── orval.config.ts
├── src/
│   ├── api/generated/       # orval output (committed, drift-checked)
│   ├── lib/                 # query client, auto-save hook (R4), i18n setup
│   ├── i18n/{de,en}.json
│   ├── components/          # shadcn/ui, unified picker, responsive table/cards, save indicator
│   ├── routes/              # TanStack Router (URL state: filters/search/current record)
│   └── features/            # dashboard, appointments, treatments, customers, patients,
│                            # pharmacy, services, templates, invoices, settings
└── tests/                   # vitest + RTL + MSW component tests

e2e/                          # Playwright (desktop + pixel-9a projects), spawns real binary
docker-compose.yml            # local Postgres
.github/workflows/ci.yml      # backend / frontend / e2e jobs + release cross-compile
```

**Structure Decision**: Web application layout using the two existing directories
(`k-vet-backend/`, `k-vet-web/`) plus a top-level `e2e/` Playwright project, matching the
one-binary deployment (release builds embed `k-vet-web/dist` via rust-embed).

## Design highlights (what implementation must respect)

- **Prices are pinned at line entry** on `treatment_item`; nothing downstream ever re-reads
  catalog prices (data-model.md). Duplication offers verbatim vs refresh (spec FR-026).
- **Stock is derived, never stored**; dispense drafts are derived from invoice status, frozen
  at acceptance, then append-only with linked compensating corrections (data-model.md).
- **Invoice numbers**: pattern from config, per-scope monotonic counter row-locked in the DB,
  numbers of accepted invoices burned (data-model.md; FR-032/033).
- **Auto-save**: create-then-PATCH with debounce and flush-on-blur (research R4); every screen;
  E2E asserts persistence across reload (SC-002).
- **Contract discipline**: handlers → utoipa → `openapi.json` → orval client; both committed;
  CI drift check fails the build (testing.md).

## Complexity Tracking

> No constitution violations — table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| — | — | — |
