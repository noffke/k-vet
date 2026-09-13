# Security

k-vet is the practice-management system for a single veterinary practice. It runs on one
Raspberry Pi on a practice network, with one user account, and is not offered as a hosted
service. There is no fleet to patch — a fix reaches the one installation when its operator
rebuilds the image.

## Reporting a vulnerability

Please report privately through GitHub's **[private vulnerability
reporting](https://github.com/noffke/k-vet/security/advisories/new)** rather than opening a
public issue, so a fix can land before the details do.

A useful report says what an attacker can reach, and from where — on the practice LAN, from the
internet, or only with the vet's own session. Expect an acknowledgement within a week; this is a
single maintainer's side project, not a staffed programme, so please size your expectations to
that.

## What is in scope

The parts worth looking at are the ones that guard real data:

- **Authentication and session handling** — `k-vet-backend/src/auth.rs` and the hand-written
  `PostgresSessionStore`. Everything under `/api/*` sits behind `require_auth`.
- **Static asset serving** — `src/static_assets.rs`. Its `resolve()` refuses path traversal and
  is unit-tested; a way past it is a real finding.
- **The database invariants** — the CHECK constraints, partial unique indexes and composite
  foreign keys in `k-vet-backend/migrations/`. A route that writes a state the schema is supposed
  to forbid is a bug worth reporting.
- **Invoice numbering and the money path** — `src/domain/invoice_number.rs`, `src/domain/money.rs`
  and `src/domain/stock.rs`. A number reused, an accepted invoice mutated in place, or a stock
  movement that loses its audit trail all matter here even without a classic memory-safety angle.
- **Attachment upload and storage**, and anything that lets one request read another record.

## What is not

- **Denial of service against the appliance.** One vet, one LAN, no public exposure by design.
  The advisories tolerated in `k-vet-backend/.cargo/audit.toml` are all of this shape, each with
  its reasoning written down.
- **The credentials in the repository.** `config.example.toml` carries a hash of the word
  `changeme`, and `e2e/fixtures/credentials.ts` carries the throwaway password the end-to-end
  suite boots with. Both are meant to be public. A real deployment's `config.toml` is never
  committed.
- **Anything requiring physical access to the Pi, or the operator's own credentials.**

## How updates reach a deployment

Dependency updates arrive as Dependabot pull requests and merge themselves when CI is green, but
nothing deploys itself: the image is built on the Pi by hand (`docs/installation.md`). A fix is
only live once its operator rebuilds.
