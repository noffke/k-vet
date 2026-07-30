#!/bin/sh
# Entrypoint of the k-vet container image.
#
# de-DE: Prüft die eingebundenen Pfade, bevor der Dienst startet, und legt fehlende
#        Vorlagen an. Alle drei Einbindungen (Konfiguration, Vorlagen, Dateien) müssen für
#        die UID/GID des Containers zugänglich sein — sonst scheitert es hier mit einer
#        klaren Meldung statt später beim ersten Upload.
# en-US: Checks the mounted paths before the service starts and seeds missing templates.
#        All three mounts (config, templates, uploaded files) must be accessible to the
#        container's uid/gid — otherwise this fails here with a clear message instead of
#        later at the first upload.
set -eu

CONFIG="${KVET_CONFIG:-/etc/k-vet/config.toml}"
TEMPLATES_DIR="${KVET_TEMPLATES_DIR:-/etc/k-vet/templates}"
DEFAULT_TEMPLATES="/usr/share/k-vet/templates"
EXAMPLE_CONFIG="/usr/share/k-vet/config.example.toml"
# The image runs as `ubuntu` (1000:1000); a compose `user:` override lands here too.
OWNER="$(id -u):$(id -g)"

fail() {
    printf 'k-vet: %s\n' "$1" >&2
    exit 1
}

warn() {
    printf 'k-vet: %s\n' "$1" >&2
}

# ── Configuration ────────────────────────────────────────────────────────────────────
[ -e "$CONFIG" ] || fail "no configuration at $CONFIG — copy $EXAMPLE_CONFIG to the host \
file mounted there and fill it in (see docs/installation.md)"
[ -r "$CONFIG" ] || fail "$CONFIG is not readable by uid:gid $OWNER (the image's 'ubuntu' \
user) — on the host run: chown $OWNER config.toml"

# ── Templates ────────────────────────────────────────────────────────────────────────
# Seed what is missing, never overwrite what the operator edited.
if [ -d "$TEMPLATES_DIR" ] && [ -w "$TEMPLATES_DIR" ]; then
    for name in invoice.typ invoice-email.txt; do
        [ -e "$TEMPLATES_DIR/$name" ] || cp "$DEFAULT_TEMPLATES/$name" "$TEMPLATES_DIR/$name"
    done
else
    warn "$TEMPLATES_DIR is missing or not writable by uid:gid $OWNER — templates were not \
seeded. Startup fails if [invoice] typst_template/email_template point into that directory; \
leave those keys empty to use the built-in templates. On the host run: chown -R $OWNER templates"
fi

# ── Uploaded files ───────────────────────────────────────────────────────────────────
# Fatal: without this directory no attachment and no invoice PDF can be stored.
attachments="$(sed -n 's/^[[:space:]]*attachments_dir[[:space:]]*=[[:space:]]*"\(.*\)".*$/\1/p' \
    "$CONFIG" | tail -n 1)"
: "${attachments:=/var/lib/k-vet/attachments}"

# The application creates the directory itself, so check the nearest existing ancestor.
probe="$attachments"
while [ ! -e "$probe" ] && [ "$probe" != "/" ]; do
    probe="$(dirname "$probe")"
done
[ -w "$probe" ] || fail "$attachments is not writable by uid:gid $OWNER (the image's \
'ubuntu' user) — on the host run: chown -R $OWNER attachments"

exec /usr/local/bin/k-vet-backend "$@"
