<!--
Sync Impact Report
- Version change: (template) → 1.0.0 (initial ratification)
- Modified principles: n/a — all placeholders filled for the first time
- Added sections: Core Principles (I–V), Technology & Platform Constraints,
  Development Workflow & Quality Gates, Governance
- Removed sections: none
- Templates:
  - ✅ .specify/templates/tasks-template.md — "Tests are OPTIONAL" note replaced
    (tests are mandatory per Principle II)
  - ✅ .specify/templates/plan-template.md — Constitution Check gate is filled per-feature
    from this file; no structural change needed
  - ✅ .specify/templates/spec-template.md — no change needed
- Other files:
  - ✅ requirements/testing.md — frontend CI job: eslint → biome + knip
    (aligns with requirements/general.md)
- Follow-up TODOs: none
-->

# k-vet Constitution

## Core Principles

### I. Code Quality & Tooling Discipline

Rust code MUST NOT use `unwrap()` or `expect()`; errors are handled or propagated
explicitly. Rust is linted with clippy (warnings denied) and formatted with rustfmt.
TypeScript MUST compile under `strict`; biome is the sole linter/formatter and knip
guards unused code and dependencies. Dependency versions in `Cargo.toml` and
`package.json` MUST be full semver. Code and technical documentation are written
in en-US.

Rationale: a single-maintainer project survives on consistency enforced by tools,
not by reviewers.

### II. Test-Backed Changes (NON-NEGOTIABLE)

Every code change MUST ship with tests. The test pyramid is fixed
(details in `requirements/testing.md`):

- Rust unit tests for pure logic — the money math (GOT factors, AMPreisV subset
  price formula, VAT, invoice number pattern).
- Backend integration tests are the workhorse: Axum tested in-process on
  `#[sqlx::test]` databases — always a real Postgres, never a mock DB, because
  invariants deliberately live in CHECK constraints, partial unique indexes, and
  composite FKs, and tests MUST exercise exactly those.
- Frontend component tests: Vitest + React Testing Library; MSW mocks kept honest
  by the OpenAPI-generated zod schemas and types.
- E2E: Playwright against the real binary and a real Postgres, in two projects —
  desktop viewport and Pixel 9a viewport.

Contract drift is a CI failure: the OpenAPI spec and the generated TS client are
committed; CI regenerates and fails on diff.

### III. User Experience Consistency

- Auto-save for all user input (Google-Docs-like), no explicit save. This is the
  top UX requirement — losing entered text is the worst failure mode — and
  auto-save persistence MUST be covered by E2E tests.
- Every screen MUST be responsive: laptop/desktop and mobile (Pixel 9a) are both
  first-class targets.
- i18n: the UI supports en-US and de-DE; de-DE is the default for user-facing text
  and documentation. No hardcoded user-facing strings. Configuration files are
  commented in both en-US and de-DE.
- Visual identity follows the color scheme of the practice's website. The UI
  SHOULD be visually pleasing, but UX and usability outrank aesthetics.

### IV. Data Integrity at the Database

- Every entity has its own sequence-based primary key.
- Database timestamps are stored with timezone.
- Domain invariants are pushed into the database (CHECK constraints, partial
  unique indexes, composite FKs) rather than living only in application code;
  integration tests exercise these constraints directly.

### V. Simplicity & Single-User Scope

- No multi-tenancy, no multi-user: a single user with login and session; user and
  password are supplied via configuration.
- Deployment is one executable (frontend embedded via rust-embed) targeting a
  Raspberry Pi; features MUST NOT require runtime services beyond PostgreSQL.
- YAGNI: prefer the simplest structure that satisfies the current requirement;
  anything beyond that MUST be justified in the plan's Complexity Tracking table.

## Technology & Platform Constraints

The stack decisions recorded in `requirements/vet-practice-webapp-tech-decisions.md`
are binding: Rust (Axum + sqlx) backend, PostgreSQL, Vite + React + shadcn/ui +
Tailwind frontend, TanStack Query for all server state, OpenAPI (utoipa) as the
single source of truth for generated TS types/hooks/zod schemas, Typst for invoice
PDFs, content-addressed filesystem storage for attachments. Deviating from a
recorded decision requires updating that document first.

## Development Workflow & Quality Gates

CI (GitHub Actions) gates every PR/push; a change is mergeable only when all
jobs pass:

- **backend**: `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo sqlx prepare --check`, `cargo test` (real Postgres service container)
- **frontend**: biome checks, knip, `tsc --noEmit`, vitest, OpenAPI/codegen
  drift check
- **e2e**: build the real binary, run Playwright (desktop + Pixel 9a projects)
  against it with a real Postgres

## Governance

This constitution supersedes ad-hoc practice. Every feature plan MUST pass the
Constitution Check gate before Phase 0 research and again after Phase 1 design;
violations are either removed or explicitly justified in Complexity Tracking.

Amendments: edit this file with an updated Sync Impact Report, bump the version
per semver (MAJOR: principle removal or incompatible redefinition; MINOR: new
principle or materially expanded guidance; PATCH: clarifications and wording),
and propagate changes to the templates under `.specify/templates/`.

The documents in `requirements/*.md` are the source of domain truth. A conflict
between a requirements document and this constitution MUST be resolved by
amending one of the two, never ignored.

**Version**: 1.0.0 | **Ratified**: 2026-07-26 | **Last Amended**: 2026-07-26
