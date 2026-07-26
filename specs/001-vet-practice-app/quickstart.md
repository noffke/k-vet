# Quickstart & Validation Guide

**Date**: 2026-07-26 | **Spec**: [spec.md](spec.md) | **Contracts**: [contracts/rest-api.md](contracts/rest-api.md)

## Prerequisites

- Rust stable toolchain (rustup), `cargo-sqlx` CLI
- Node.js 22.12+ (or 20.19+ LTS) and npm — Vite 8 engines requirement (`^20.19.0 || >=22.12.0`)
- Docker (local Postgres only — tests and dev use a real Postgres, never a mock)

## Development setup

```bash
# 1. Database
docker compose up -d db                     # Postgres on localhost:5432

# 2. Backend (from k-vet-backend/)
cp ../config.example.toml config.toml       # commented in de-DE + en-US; set user/password hash
cargo sqlx migrate run                      # includes the GOT 2022 import migration
cargo run                                   # API on :8080, serves embedded frontend in release

# 3. Frontend (from k-vet-web/, dev mode with proxy to :8080)
npm ci
npm run dev
```

Contract regeneration after handler changes (both outputs are committed; CI fails on drift):

```bash
cargo run --bin export-openapi > openapi.json   # utoipa spec
cd ../k-vet-web && npm run generate:api          # orval → src/api/generated/
```

## Test commands

```bash
# Backend: unit (money math) + integration (#[sqlx::test], real Postgres)
cd k-vet-backend && cargo fmt --check && cargo clippy -- -D warnings \
  && cargo sqlx prepare --check && cargo test

# Frontend: lint/format/dead-code + types + component tests
cd k-vet-web && npx biome check && npx knip && npx tsc --noEmit && npm test

# E2E: real binary + real Postgres, desktop + Pixel 9a viewports
cd e2e && npx playwright test
```

## End-to-end validation scenarios

Each scenario maps to spec success criteria (SC-xxx) and is automated in Playwright unless
marked manual.

1. **Core loop** (SC-001): create customer + patient → new appointment (today, set time) →
   treatment → add a stocked drug line and a GOT service line via the picker → create invoice
   (Includes Finding) → inspect PDF → accept with an email recipient → invoice is `accepted`,
   PDF downloadable, dispense movements frozen.
2. **Auto-save** (SC-002): type into treatment finding and a customer field, reload the page
   without any save action → text is present. Kill the dev server mid-typing and restart →
   last debounced state is present.
3. **Picker ranking** (SC-003): after several treatments using drug X, search a common prefix →
   drug X ranks above less-used matches; drugs and services show distinct icons.
4. **Batch traceability** (SC-004): from a lot detail, every dispense row links to its
   treatment/invoice/customer; a cancelled invoice shows compensating corrections linked to the
   dispenses they reverse.
5. **Money math** (SC-005, unit-level): AMPreisV worked examples (original + subset packaging),
   Wegegeld `max(km × 3.50, 13.00)` cases incl. multiplier, VAT extraction per rate, invoice
   number pattern — reviewed by the vet, pinned in `cargo test`.
6. **Invoice number discipline** (SC-006): update a `created` invoice → number kept; update an
   `accepted` invoice → auto-cancel, number burned, new number issued; counter never reused
   across restarts (integration test).
7. **Stock reconciliation** (SC-007): intake 2 packages × 100 ml → dispense 30 ml (FEFO split
   when lot 1 has < 30 remaining) → accept invoice → cancel invoice → remaining stock equals
   initial again; movement history append-only.
8. **Bookkeeping hand-off** (SC-008): with 3 accepted invoices, bulk-download returns all 3
   PDFs; bulk-submit marks them; dashboard counter drops to 0 and links to the list.
9. **Responsive** (SC-009): all Playwright scenarios run in both viewport projects; tables
   render as stacked cards on Pixel 9a; no horizontal scrolling.
10. **i18n** (SC-010): app boots in de-DE; switching to en-US translates all visible strings
    (spot-checked per screen); config file comments bilingual (manual review).
11. **Warnings & archiving**: customer with warning remark shows prominent banner on open and
    icon+tooltip in lists; setting date of death archives the patient; archived entries hidden
    from picker/search until "show archived" is toggled.
12. **Release build** (manual, before deploy): `cargo zigbuild --target aarch64-unknown-linux-gnu --release`
    with embedded frontend; binary serves UI + API on one port on the Pi; systemd unit starts it;
    `/metrics` scraped by the existing Prometheus; borgmatic snapshot includes DB dump +
    attachments dir; one full test restore (open a photo) before trusting it.
