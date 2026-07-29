#!/usr/bin/env bash
# Fails when the committed OpenAPI document or the generated TypeScript client differ
# from what the current handlers produce (Constitution II: contract drift is a CI failure).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

spec="k-vet-backend/openapi.json"
generated="k-vet-web/src/api/generated"

echo "==> regenerating $spec"
(cd k-vet-backend && SQLX_OFFLINE=true cargo run --quiet --bin export-openapi) > "$spec.check"

if ! diff -u "$spec" "$spec.check" > /dev/null; then
    echo "ERROR: $spec is out of date. Run:" >&2
    echo "  cd k-vet-backend && cargo run --bin export-openapi > openapi.json" >&2
    diff -u "$spec" "$spec.check" | head -50 >&2
    rm -f "$spec.check"
    exit 1
fi
rm -f "$spec.check"

echo "==> regenerating $generated"
before="$(mktemp -d)"
cp -r "$generated" "$before/generated"
(cd k-vet-web && npm run --silent generate:api)

if ! diff -r -u "$before/generated" "$generated"; then
    echo "ERROR: the generated API client is out of date. Run:" >&2
    echo "  cd k-vet-web && npm run generate:api" >&2
    rm -rf "$before"
    exit 1
fi
rm -rf "$before"

echo "OK: OpenAPI document and generated client are up to date"
