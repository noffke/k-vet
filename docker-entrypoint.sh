#!/bin/sh
# Entrypoint of the k-vet container image.
#
# de-DE: Legt fehlende Vorlagen im eingebundenen Vorlagenverzeichnis an und startet dann die
#        Anwendung. Geprüft wird hier nichts: Konfiguration, Dateiverzeichnis und Datenbank
#        prüft die Anwendung selbst, mit demselben Parser, mit dem sie sie auch benutzt.
# en-US: Seeds missing templates into the mounted templates directory, then starts the
#        application. Nothing is checked here: the configuration, the attachments directory and
#        the database are checked by the application itself, with the same parser it uses them
#        with.
#
# That last point is deliberate. This script used to preflight the configuration and the
# attachments directory too, which made it a second implementation of "what does startup
# need" — and it drifted: it demanded a configuration before `--hash-password`, the command
# that helps write one, and it found `attachments_dir` with `sed`, which reads a valid
# single-quoted TOML path as no path at all and so checked the wrong directory. One owner, in
# the binary, cannot disagree with itself.
set -eu

TEMPLATES_DIR="${KVET_TEMPLATES_DIR:-/etc/k-vet/templates}"
DEFAULT_TEMPLATES="/usr/share/k-vet/templates"

# Seed what is missing, never overwrite what the operator edited. This copies files out of the
# image into a mount and decides nothing, which is why it is the one job that stays here.
if [ -d "$TEMPLATES_DIR" ] && [ -w "$TEMPLATES_DIR" ]; then
    for name in invoice.typ invoice-email.txt; do
        [ -e "$TEMPLATES_DIR/$name" ] || cp "$DEFAULT_TEMPLATES/$name" "$TEMPLATES_DIR/$name"
    done
else
    printf 'k-vet: %s is missing or not writable by uid:gid %s — templates were not seeded; on the host run: chown -R %s templates\n' \
        "$TEMPLATES_DIR" "$(id -u):$(id -g)" "$(id -u):$(id -g)" >&2
fi

exec /usr/local/bin/k-vet-backend "$@"
