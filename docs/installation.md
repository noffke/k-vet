# Installing k-vet

k-vet is deployed as a container image holding the backend binary and the built web interface,
plus a PostgreSQL container. The image is **built on the machine that runs it** — nothing is
pulled from a registry.

This guide assumes a 64-bit Raspberry Pi OS or any Linux host with Docker Engine 23 or newer
(for BuildKit) and the compose plugin:

```bash
docker --version && docker compose version
```

## 1. Get the sources

```bash
git clone <this repository> k-vet && cd k-vet
```

Everything below runs from that directory. The two files that matter for a deployment are
`Dockerfile` and `docker-compose.deploy.yml`.

## 2. Prepare the host files

Three paths are mounted into the container, and they are the only state that lives outside the
database:

| Host path | Container path | Contents | Access needed |
| --- | --- | --- | --- |
| `./config.toml` | `/etc/k-vet/config.toml` | operator configuration | read |
| `./templates/` | `/etc/k-vet/templates` | invoice PDF and email templates | read + write |
| `./attachments/` | `/var/lib/k-vet/attachments` | uploaded files, invoice PDFs, logo | read + write |

**The container runs as the image's `ubuntu` user — uid 1000 and gid 1000 — so all three paths
must be accessible to that uid and gid.** On Raspberry Pi OS the first login user is already
1000:1000, so a checkout made by that user needs nothing further. Otherwise:

```bash
mkdir -p templates attachments
cp config.example.toml config.toml
sudo chown -R 1000:1000 config.toml templates attachments
ls -n                # owner and group of all three must read 1000 1000
```

Getting this wrong shows up as one of three things, all of them loud except the last:

- the container exits at startup saying `config.toml` is not readable by uid:gid 1000:1000,
- it exits saying the attachments directory is not writable — uploads and invoice PDFs would
  fail at the first use,
- `templates/` stays empty and a warning says seeding was skipped. What happens next depends on
  the configuration: with `typst_template`/`email_template` pointing into that directory the
  container then exits naming the missing file; with those keys empty the compiled-in templates
  are used and only your ability to edit them is lost.

A `user:` override in the compose service works if 1000 is taken on your host, but then the same
ownership rule applies to whichever uid/gid you substitute.

## 3. Write the configuration

`config.example.toml` documents every option in German and English. Three values differ from a
bare-metal setup and must be right for the container:

```toml
[server]
listen_address = "0.0.0.0"          # the port is published by compose, so bind all interfaces

[database]
url = "postgres://kvet:PASSWORD@db:5432/kvet"   # `db` is the compose service name

[storage]
attachments_dir = "/var/lib/k-vet/attachments"  # the mount, not a host path
```

The remaining decisions:

| Section | Key | What to set |
| --- | --- | --- |
| `[server]` | `port`, `base_url` | Keep 8080 (the published port) and set `base_url` to the URL the vet types. An `https://` value also switches the session cookie to `Secure`. |
| | `web_dir` | Leave at `/usr/share/k-vet/web` — that is where the image puts the built interface. |
| | `log_level` | `info`; `debug` adds request and SQL detail. |
| `[auth]` | `username`, `password_hash` | The single login. Generate the hash after the first build: `docker run --rm -i k-vet:local --hash-password` (type the password, it prints an argon2id hash). Never store the plain password. |
| `[database]` | `max_connections` | 5 is plenty for one user. |
| `[storage]` | `max_upload_mb` | Larger uploads are rejected with 413. |
| `[mail]` | `smtp_*`, `from_*` | The practice's own mail server; `smtp_tls` is `starttls`, `tls` or `none`. |
| `[invoice]` | `number_pattern` | e.g. `{year}-{counter:4}` → `2026-0042`. The counter's scope follows the date placeholders: with `{year}` it restarts yearly, with `{year}`+`{month}` monthly, without any date part it runs forever. It never decrements and a number is never reissued. |
| | `typst_template`, `email_template` | Empty uses the compiled-in templates and leaves the mount inert. To edit them, point at `/etc/k-vet/templates/invoice.typ` and `/etc/k-vet/templates/invoice-email.txt` — the entrypoint seeds both files on first start. See [templates.md](templates.md). |
| `[travel_expenses]` | `rate_per_double_km`, `minimum` | GOT § 10 values: `3.50` per double kilometre, `13.00` minimum. Update when the fee schedule changes. |

The database password appears twice — in `config.toml` and in the compose environment. Put it in
an `.env` file next to the compose file so the second copy is not in the shell history:

```bash
echo "KVET_DB_PASSWORD=$(openssl rand -hex 16)" > .env
```

Practice name, address, IBAN, VAT ID, logo and the global CC/BCC addresses are **not** in this
file — the vet edits them in the application under *Einstellungen*.

## 4. Build and start

```bash
docker compose -f docker-compose.deploy.yml build      # first build on a Pi: expect 30–60 min
docker compose -f docker-compose.deploy.yml up -d
docker compose -f docker-compose.deploy.yml ps         # app must become healthy
curl -fsS localhost:8080/healthz                       # → ok
```

The first build compiles the whole Rust dependency tree, Typst included; give the Pi swap
headroom (2 GB is comfortable) and let it run. Rebuilds reuse BuildKit's cargo and target caches
and take minutes. Schema migrations run automatically every time the container starts, so there
is no separate migration step.

Logs: `docker compose -f docker-compose.deploy.yml logs -f app`.

## 5. First login

Open `base_url` in a browser and sign in with the credentials from `[auth]`. Then, in this order:

1. **Einstellungen** — practice name, address, IBAN, VAT ID, logo. These appear on every invoice
   generated afterwards, so fill them in before billing anything.
2. **Leistungen** — the GOT 2022 catalogue is already there (surgical positions hidden). Add the
   practice's own positions, including a travel-expenses position if house calls are billed.
3. **Apotheke** — suppliers, manufacturers, drugs with their packagings, then a stock intake per
   lot.

The interface is German by default; `de`/`en` in the corner switches languages, and numbers,
dates and money follow the chosen locale on input as well as on display.

## Monitoring

- `GET /healthz` — liveness, no session required. Also the image's `HEALTHCHECK`.
- `GET /metrics` — Prometheus text format: request counts and latency histograms labelled by
  route pattern. Unauthenticated, so keep the published port off the open internet or restrict
  `/metrics` in a reverse proxy.

Nightly at 03:00 the service refreshes the picker's usage weights and deletes abandoned drafts
(opened, never filled in, older than a day). On the first of each month it also removes uploaded
files nothing references any more.

## Backups

**Back up the database dump and the attachments directory together, from the same point in
time.** Either one alone is incomplete: the database holds the metadata and every invoice's
content, the directory holds the bytes of the PDFs and patient files they point at. The mounted
`config.toml` and `templates/` are worth keeping too — they are small and hold your setup.

```bash
docker compose -f docker-compose.deploy.yml exec -T db \
  pg_dump -U kvet --format=custom kvet > kvet-$(date +%F).dump
tar -czf kvet-files-$(date +%F).tar.gz attachments templates config.toml
```

Restoring: start only the database (`up -d db`), `pg_restore` into it, unpack the files
alongside the compose file, then `up -d app`. Verify a restore before you need it — open an old
invoice's PDF and check a patient file opens.

## Upgrading

```bash
git pull
docker compose -f docker-compose.deploy.yml build
docker compose -f docker-compose.deploy.yml up -d
```

Migrations run at start and are forward-only; take a backup first (see above) and watch
`docker compose -f docker-compose.deploy.yml logs -f app` on the first start after an upgrade.
Old images can be cleaned up with `docker image prune`.

## Running without Docker

Not supported as a deployment. The binary alone works for development —
`cargo run` in `k-vet-backend/` with `npm run dev` in `k-vet-web/` — but a production install
needs the built interface on disk and the paths above, which is exactly what the image provides.
