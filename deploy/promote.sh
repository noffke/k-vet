#!/usr/bin/env bash
#
# Puts a released version onto one instance, on the Pi.
#
#   deploy/promote.sh staging 1.2.3      try it
#   deploy/promote.sh prod    1.2.3      then the same image, unchanged
#
# The image is built once and both instances run that one build, so what the practice gets is
# what was tried — not a rebuild of the same source. Building is the slow part on a Pi; this
# skips it when the image is already there, which is exactly the case when promoting staging
# to production.
#
# It takes a dump before touching production and says so. Migrations are forward-only and run
# at start, so once a release with a migration has started there is no going back to the old
# image — only forward, or a restore. That dump is the whole of the rollback plan.
set -euo pipefail

die() {
    printf 'promote: %s\n' "$1" >&2
    exit 1
}

instance="${1:-}"
version="${2:-}"
[ -n "$instance" ] && [ -n "$version" ] ||
    die "usage: deploy/promote.sh <prod|staging> <version>"
version="${version#v}"

case "$instance" in
    prod | staging) ;;
    *) die "'$instance' is not an instance — expected prod or staging" ;;
esac

repo="$(cd "$(dirname "$0")/.." && pwd)"
env_file="/etc/k-vet/${instance}.env"
image="k-vet:${version}"

[ -f "$env_file" ] || die "no environment file at $env_file"

# ── the image ────────────────────────────────────────────────────────────────────────────────
if docker image inspect "$image" >/dev/null 2>&1; then
    echo "==> $image is already built"
else
    echo "==> building $image (minutes, or the better part of an hour on a cold cache)"
    [ -z "$(git -C "$repo" status --porcelain)" ] ||
        die "the checkout at $repo is dirty; the image would not be the released source"
    git -C "$repo" fetch --tags --quiet
    git -C "$repo" rev-parse -q --verify "refs/tags/v${version}" >/dev/null ||
        die "no tag v${version} — is it pushed, and has CI finished with it?"
    git -C "$repo" checkout --quiet "v${version}"
    docker build --build-arg "KVET_VERSION=${version}" -t "$image" "$repo"
fi

# ── the safety net, for production only ──────────────────────────────────────────────────────
if [ "$instance" = "prod" ]; then
    dump="/srv/k-vet/backups/kvet-$(date +%Y%m%dT%H%M%S)-before-${version}.dump"
    mkdir -p "$(dirname "$dump")"
    echo "==> dumping the production database first"
    sudo -u postgres pg_dump --format=custom kvet >"$dump"
    echo "    $dump"
fi

# ── the one line that decides what runs ──────────────────────────────────────────────────────
previous="$(grep '^KVET_VERSION=' "$env_file" | cut -d= -f2- || true)"
echo "==> $instance: ${previous:-unset} -> ${version}"
sudo sed -i "s|^KVET_VERSION=.*|KVET_VERSION=${version}|" "$env_file"
grep -q "^KVET_VERSION=${version}$" "$env_file" || die "could not rewrite $env_file"

sudo systemctl restart "k-vet@${instance}"

# ── did it come back ─────────────────────────────────────────────────────────────────────────
port="$(grep '^KVET_HTTP_PORT=' "$env_file" | cut -d= -f2-)"
port="${port:-8080}"
echo "==> waiting for http://127.0.0.1:${port}/healthz"
for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:${port}/healthz" >/dev/null 2>&1; then
        # /metrics is unauthenticated on the appliance's own port, and carries the build that
        # actually answered — which is the point: not the tag that was typed, the one running.
        running="$(curl -fsS "http://127.0.0.1:${port}/metrics" |
            sed -n 's/.*kvet_build_info{version="\([^"]*\)".*/\1/p' | head -1)"
        echo "==> $instance is up, running ${running:-unknown}"
        [ "$running" = "$version" ] ||
            echo "    WARNING: expected ${version} — check ${env_file} and the image tag" >&2
        exit 0
    fi
    sleep 2
done

printf '\n' >&2
die "$instance did not become healthy in two minutes.
  journalctl -u k-vet@${instance} -n 50
$(if [ "$instance" = "prod" ]; then
        printf '  If a migration ran, the old image will not start against the new schema.\n'
        printf '  Restore first:  sudo -u postgres pg_restore -d kvet --clean %s\n' "${dump:-<the dump above>}"
    fi)"
