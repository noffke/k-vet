#!/usr/bin/env bash
#
# Cuts a release: checks, tags, pushes. CI does the rest — it re-runs every gate on the tagged
# commit, verifies the image builds, and publishes the GitHub release.
#
#   scripts/cut-release.sh 1.2.3
#
# Run on the development machine, never on the Pi. The Pi consumes tags; it does not make them.
#
# Every check here exists because a tag is the one thing in this project that is meant to be
# permanent: the Pi builds from it, the release notes are generated from it, and
# `git log <last deployed tag>..main` is how anyone works out what the practice has not seen
# yet. A tag on a dirty tree or a red commit quietly poisons all three.
set -euo pipefail

die() {
    printf 'cut-release: %s\n' "$1" >&2
    exit 1
}

version="${1:-}"
[ -n "$version" ] || die "usage: scripts/cut-release.sh 1.2.3"
# Accept 1.2.3 or v1.2.3; the tag is always the v form.
version="${version#v}"
tag="v${version}"

case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) die "'$version' is not a semver version — expected something like 1.2.3" ;;
esac

cd "$(dirname "$0")/.."

[ -z "$(git status --porcelain)" ] ||
    die "the working tree has uncommitted changes; a tag must name something that exists"

branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$branch" = "main" ] || die "on branch '$branch'; releases are cut from main"

git rev-parse -q --verify "refs/tags/$tag" >/dev/null &&
    die "tag $tag already exists locally"

# The remote is asked first and its failure is fatal. Every check below reads the remote's
# answer, and an unreachable remote answers "no" to all of them — "that tag does not exist yet"
# is indistinguishable from "I could not ask", and only one of those is safe to act on.
echo "fetching…"
git fetch --quiet origin main ||
    die "cannot reach origin; a release is checked against the remote, so this is not skippable"

[ -z "$(git ls-remote --tags origin "refs/tags/$tag")" ] ||
    die "tag $tag already exists on the remote"

head="$(git rev-parse HEAD)"
[ "$head" = "$(git rev-parse origin/main)" ] ||
    die "main is not level with origin/main — pull or push first"

# The tag must land on a commit CI has already judged. Without this the first anyone hears of a
# failure is the tag's own run, by which point the tag is public and immutable.
echo "checking CI for ${head:0:8}…"
conclusion="$(gh run list --workflow=ci.yml --branch main --limit 30 \
    --json headSha,conclusion --jq "[.[] | select(.headSha == \"$head\")] | .[0].conclusion")"
case "$conclusion" in
    success) ;;
    "" | null) die "no finished CI run for ${head:0:8} — wait for it, or check with: gh run list" ;;
    *) die "CI concluded '$conclusion' for ${head:0:8}; fix that before tagging" ;;
esac

# Named in the tag message so `git show <tag>` says whether this release can be rolled back by
# restarting the old image, or needs a restore. The Pi operator reads this before promoting.
previous="$(git describe --tags --abbrev=0 2>/dev/null || true)"
if [ -n "$previous" ]; then
    migrations="$(git diff --name-only "$previous..HEAD" -- k-vet-backend/migrations/)"
else
    migrations="$(git ls-files k-vet-backend/migrations/)"
fi
if [ -n "$migrations" ]; then
    note="Contains migrations — rolling back needs a restore, not just the previous image:"
    note="$note"$'\n'"$(echo "$migrations" | sed 's|k-vet-backend/migrations/|  |')"
else
    note="No migrations — this release can be rolled back by pointing the env file back."
fi

printf '\n%s\n%s\n\n' "$tag" "$note"
printf 'tag and push? [y/N] '
read -r answer
case "$answer" in [yY]*) ;; *) die "nothing done" ;; esac

git tag -a "$tag" -m "$tag" -m "$note"
git push origin "$tag"

cat <<EOF

Pushed $tag. CI is now running every gate on it and will publish the release.

  gh run watch
  gh release view $tag

Then, on the Pi:

  deploy/promote.sh staging $version
EOF
