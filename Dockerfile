# k-vet container image — the deployment artefact, built on the target machine.
#
# de-DE: Drei Stufen: Frontend bauen (Node), Backend bauen (Rust), fertiges Abbild auf
#        Ubuntu. Das Frontend wird als Dateien mitgeliefert und vom Backend ausgeliefert
#        (`server.web_dir`), nicht mehr in die Binärdatei eingebettet.
# en-US: Three stages: build the frontend (Node), build the backend (Rust), assemble the
#        runtime image on Ubuntu. The frontend ships as files and is served by the backend
#        (`server.web_dir`) instead of being embedded in the binary.
#
#   docker compose -f docker-compose.deploy.yml build
#
# BuildKit is required for the cache mounts below (default in Docker 23+).

# ── Frontend ──────────────────────────────────────────────────────────────────────────
FROM node:26.8-bookworm-slim AS web
WORKDIR /build

# Dependencies first: they only change when the lockfile does.
COPY k-vet-web/package.json k-vet-web/package-lock.json ./
RUN npm ci

COPY k-vet-web/ ./
# Also type-checks: `npm run build` is `tsc --noEmit && vite build`.
RUN npm run build

# ── Backend ───────────────────────────────────────────────────────────────────────────
FROM rust:1.97.1-bookworm AS backend
WORKDIR /build

# No database is reachable during the build, so sqlx uses the committed `.sqlx/` metadata.
ENV SQLX_OFFLINE=true

COPY k-vet-backend/Cargo.toml k-vet-backend/Cargo.lock k-vet-backend/clippy.toml ./
COPY k-vet-backend/.sqlx ./.sqlx
COPY k-vet-backend/src ./src
COPY k-vet-backend/migrations ./migrations
COPY k-vet-backend/templates ./templates
# The invoice sets in Source Sans 3, which `src/pdf` pulls in with `include_bytes!` — without
# these the release build fails at compile time, and only on a `v*` tag where the image is built.
COPY k-vet-backend/fonts ./fonts

# The binary is copied out inside this RUN: a cache-mounted `target/` is gone afterwards.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --locked \
    && mkdir -p /out && cp target/release/k-vet-backend /out/k-vet-backend

# ── Runtime ───────────────────────────────────────────────────────────────────────────
# The Debian-based builder links against glibc 2.36, which runs on noble's 2.39; the other
# way round would not work.
FROM ubuntu:noble

# ca-certificates for outgoing SMTP over TLS, curl only for the health check below. The
# invoice PDF fonts are compiled into the binary, so no font packages are needed.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend /out/k-vet-backend /usr/local/bin/k-vet-backend
COPY --from=web /build/dist /usr/share/k-vet/web
# Shipped defaults. The entrypoint seeds the mounted templates directory from here.
COPY k-vet-backend/templates/ /usr/share/k-vet/templates/
COPY config.example.toml /usr/share/k-vet/config.example.toml
# The font is compiled into the binary, so the licence has to travel with the image: SIL OFL 1.1
# asks that the notice accompany every copy of the Font Software.
COPY k-vet-backend/fonts/LICENSE.txt /usr/share/k-vet/licences/SourceSans3-OFL.txt
COPY docker-entrypoint.sh /usr/local/bin/kvet-entrypoint.sh

# Runs as noble's own `ubuntu` user (uid 1000, gid 1000) — the same ids as the first login
# user on Raspberry Pi OS, so bind-mounted host directories are writable without a chown.
# Mount points are pre-created so a named volume inherits the right ownership.
RUN chmod +x /usr/local/bin/kvet-entrypoint.sh \
    && install -d -o ubuntu -g ubuntu -m 750 \
       /etc/k-vet /etc/k-vet/templates /var/lib/k-vet /var/lib/k-vet/attachments

ENV KVET_CONFIG=/etc/k-vet/config.toml \
    KVET_TEMPLATES_DIR=/etc/k-vet/templates

EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

USER ubuntu
ENTRYPOINT ["/usr/local/bin/kvet-entrypoint.sh"]
