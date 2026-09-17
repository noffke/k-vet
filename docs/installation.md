# Installing k-vet

k-vet is deployed as a container image holding the backend binary and the built web interface,
against the PostgreSQL server the host already runs. The image is **built on the machine that
runs it** — nothing is pulled from a registry.

Instances are systemd units, `k-vet@prod` and `k-vet@staging`, so the same machine can run the
practice's own installation and a place to try a release first. Everything below is written for
one instance called `prod`; a second is the same steps with a different name, a different port
and a different database. If you only ever want one, use `prod` and ignore the rest.

This guide assumes a 64-bit Raspberry Pi OS or any Linux host with Docker Engine 23 or newer
(for BuildKit), systemd, and PostgreSQL 15 or newer:

```bash
docker --version && systemctl --version | head -1 && psql --version
```

## 1. Get the sources

```bash
git clone <this repository> k-vet && cd k-vet
```

Everything below runs from that directory. The files that matter for a deployment are
`Dockerfile`, `deploy/k-vet@.service`, `deploy/env.example` and `deploy/promote.sh`.

Keep the checkout somewhere stable — `deploy/promote.sh` builds from it, checking out the tag
it was asked for.

## 2. Prepare the host files

Three paths are mounted into the container, and they are the only state that lives outside the
database:

| Host path | Container path | Contents | Access needed |
| --- | --- | --- | --- |
| `/srv/k-vet/prod/config.toml` | `/etc/k-vet/config.toml` | operator configuration | read |
| `/srv/k-vet/prod/templates/` | `/etc/k-vet/templates` | invoice PDF and email templates | read + write |
| `/srv/k-vet/prod/attachments/` | `/var/lib/k-vet/attachments` | uploaded files, invoice PDFs, logo | read + write |

Put them on the USB SSD, not the SD card — the same rule as the database.

**A second instance gets its own three, under `/srv/k-vet/staging/`, and the attachments
directory in particular must not be shared.** That is not tidiness: attachment storage is
content-addressed and deduplicated, and the nightly sweep decides what to delete by asking its
*own* database what is still referenced. Two instances over one directory would let the test one
quietly delete files the practice's invoices point at.

**The container runs as the image's `ubuntu` user — uid 1000 and gid 1000 — so all three paths
must be accessible to that uid and gid.** On Raspberry Pi OS the first login user is already
1000:1000, so a checkout made by that user needs nothing further. Otherwise:

```bash
sudo mkdir -p /srv/k-vet/prod/templates /srv/k-vet/prod/attachments
sudo cp config.example.toml /srv/k-vet/prod/config.toml
sudo chown -R 1000:1000 /srv/k-vet/prod
ls -n /srv/k-vet/prod    # owner and group of all three must read 1000 1000
```

Getting this wrong shows up as one of three things, all of them loud except the last:

- the container exits at startup saying `config.toml` is not readable by uid:gid 1000:1000,
- it exits saying the attachments directory is not writable — uploads and invoice PDFs would
  fail at the first use,
- `templates/` stays empty and a warning says seeding was skipped. What happens next depends on
  the configuration: with `typst_template`/`email_template` pointing into that directory the
  container then exits naming the missing file; with those keys empty the compiled-in templates
  are used and only your ability to edit them is lost.

Adding `--user` to the unit's `docker run` works if 1000 is taken on your host, but then the same
ownership rule applies to whichever uid/gid you substitute.

## 3. Write the configuration

`config.example.toml` documents every option in German and English. Three values differ from a
bare-metal setup and must be right for the container:

```toml
[server]
listen_address = "0.0.0.0"          # inside the container; the unit publishes the host port

[database]
url = "postgres://kvet:PASSWORD@host.docker.internal:5432/kvet"   # the host's own server

[storage]
attachments_dir = "/var/lib/k-vet/attachments"  # the mount, not a host path
```

The remaining decisions:

| Section | Key | What to set |
| --- | --- | --- |
| `[server]` | `port`, `base_url` | Keep 8080 (the published port) and set `base_url` to the URL the vet types. An `https://` value also switches the session cookie to `Secure`. |
| | `web_dir` | Leave at `/usr/share/k-vet/web` — that is where the image puts the built interface. |
| | `log_level` | `info`; `debug` adds request and SQL detail. |
| `[auth]` | `username`, `password_hash` | The single login. Generate the hash after the first build: `docker run --rm -i k-vet:1.0.0 --hash-password` (type the password, it prints an argon2id hash). Never store the plain password. |
| `[database]` | `max_connections` | 5 is plenty for one user. |
| `[storage]` | `max_upload_mb` | Larger uploads are rejected with 413. |
| `[mail]` | `smtp_*`, `from_*` | The practice's own mail server; `smtp_tls` is `starttls`, `tls` or `none`. |
| `[invoice]` | `number_pattern` | e.g. `{year}-{counter:4}` → `2026-0042`. The counter's scope follows the date placeholders: with `{year}` it restarts yearly, with `{year}`+`{month}` monthly, without any date part it runs forever. It never decrements and a number is never reissued. |
| | `typst_template`, `email_template` | Empty uses the compiled-in templates and leaves the mount inert. To edit them, point at `/etc/k-vet/templates/invoice.typ` and `/etc/k-vet/templates/invoice-email.txt` — the entrypoint seeds both files on first start. See [templates.md](templates.md). |
| `[travel_expenses]` | `rate_per_double_km`, `minimum` | GOT § 10 values: `3.50` per double kilometre, `13.00` minimum. Update when the fee schedule changes. |

The database password lives here and nowhere else — it is the role's password from
[section 4](#4-the-database), and this file is the only copy. `chmod 600` it; the container
reads it as uid 1000.

The instance's **time zone** is not in this file but in `/etc/k-vet/<instance>.env`, and it
defaults to `Europe/Berlin`. It is not cosmetic: the application asks the system what day it is
for the **invoice date**, an intake's arrival date, today's Termine and the nightly job, so a
container left on UTC dates an invoice written after midnight to the previous day — and, if
`invoice.number_pattern` carries date parts, to that day's counter. Set `KVET_TZ=` there for a
practice somewhere else.

A staging instance also sets `environment_label` (a banner, so it cannot be mistaken for the
practice's own) and should point `[mail] smtp_host` at something dead.

Practice name, address, e-mail, bank details (IBAN, BIC, bank name), VAT ID, logo and the global
CC/BCC addresses are **not** in this file — the vet edits them in the application under
*Einstellungen*.

## 4. The database

k-vet uses the PostgreSQL server the host already runs; it does not bring one. Put its data
directory on the **USB SSD, never the SD card** — write-ahead logging destroys SD cards.

### The role and the database

Create a login role and a database it owns. Ownership is the simplest way to give the application
the rights it needs; the alternative is two explicit grants, below.

```sql
CREATE ROLE kvet LOGIN PASSWORD 'the-password-from-your-config';
CREATE DATABASE kvet OWNER kvet ENCODING 'UTF8';
```

**One role per instance, each owning its own database**, so a mistake in one instance's
`config.toml` cannot open the other's data:

```sql
CREATE ROLE kvet_staging LOGIN PASSWORD 'the-password-from-the-staging-config';
CREATE DATABASE kvet_staging OWNER kvet_staging ENCODING 'UTF8';
```

Invoice numbers are allocated per database and never reused, so two instances must never share
one — quite apart from the test one then billing against the practice's counter.

### Letting the container in

The application runs in a container and PostgreSQL does not, so the connection arrives over the
Docker bridge — which a default installation does not accept. Two edits, then a reload:

```conf
# postgresql.conf — confirm the address with `ip addr show docker0`
listen_addresses = 'localhost,172.17.0.1'
```

```conf
# pg_hba.conf — role and database names match, so neither instance can reach the other's
host  kvet          kvet          172.16.0.0/12  scram-sha-256
host  kvet_staging  kvet_staging  172.16.0.0/12  scram-sha-256
```

```bash
sudo systemctl reload postgresql
```

Keep 5432 off the network itself; `listen_addresses` above is the loopback and the bridge, not
`*`.

**The encoding must be UTF-8.** Every practice name, patient name and clinical note is German text,
and the invoice PDF is rendered from what the database returns.

That is the whole setup. Do **not** grant `SUPERUSER`, `CREATEDB` or `CREATEROLE` — none is
required, as the section below explains.

### If the role cannot own the database

Where the database already exists and belongs to someone else, two grants are enough:

```sql
GRANT CREATE ON DATABASE kvet TO kvet;   -- for CREATE EXTENSION and CREATE SCHEMA
\connect kvet
GRANT ALL ON SCHEMA public TO kvet;      -- for the tables, types, functions and triggers
```

The second one is easy to miss. Since **PostgreSQL 15** the `public` schema is no longer writable
by everyone, so a role with only `CONNECT` fails on the very first migration with
`permission denied for schema public` — even though it can log in perfectly well.

### What the migrations create, and why those rights

| The migrations run | Needs | Where |
| --- | --- | --- |
| `CREATE EXTENSION IF NOT EXISTS pg_trgm` | `CREATE` on the **database** | `0001` — trigram indexes behind customer, patient and drug search |
| `CREATE SCHEMA IF NOT EXISTS tower_sessions` | `CREATE` on the **database** | `0007` — the login session store |
| tables, enum types, functions, triggers, indexes | `CREATE` on schema **public** | all migrations |

`pg_trgm` is a *trusted* extension from PostgreSQL 13 onwards, which is why installing it needs no
superuser — only `CREATE` on the database. On PostgreSQL 12 or older it would, but k-vet is
developed and tested against PostgreSQL 16.

### The connection URL

```toml
[database]
url = "postgres://kvet:PASSWORD@host.docker.internal:5432/kvet"
max_connections = 5
```

`host.docker.internal` is the host itself; the unit supplies it with
`--add-host=host.docker.internal:host-gateway`. A staging instance points at
`kvet_staging` with its own role and password.

`db` is the compose service name; for an external server use its host name or address. **Percent-encode
any special character in the password** — `@`, `/`, `:`, `?`, `#` and `%` all mean something in a
URL, so a password of `p@ss/wort` becomes `p%40ss%2Fwort`. A password that ends up splitting the URL
usually shows as a host-not-found or database-not-found error rather than an authentication one,
which sends you looking in the wrong place. Generating the password with `openssl rand -hex 16`, as
section 3 suggests, avoids the question entirely.

`max_connections = 5` is ample for a single user. It is a *pool* size: the server's own
`max_connections` must be at least this plus whatever else uses it.

### Migrations apply themselves

The application runs its migrations at every start, so there is no separate step and no `sqlx-cli`
on the server. A migration that fails aborts start-up rather than leaving the schema half-applied,
and the error names the migration.

This also means **the database must be reachable before the application starts**. The compose file
handles that with a health check; against an external server, make sure it is up first.

### Checking it worked

```bash
psql "postgres://kvet:PASSWORD@localhost:5432/kvet" -c "\dt"        # 22 tables
psql "postgres://kvet:PASSWORD@localhost:5432/kvet" \
     -c "select count(*) from _sqlx_migrations;"                   # one row per migration
psql "postgres://kvet:PASSWORD@localhost:5432/kvet" \
     -c "select count(*) from service where type = 'got';"          # 931 GOT positions
```

The GOT fee schedule is imported by a migration, so a fresh database already has all 931 positions.

### Starting over

The application cannot drop its own schema, deliberately — emptying a database is the database's
job, and a flag inside the application knows only *that* a reset was allowed, never *which*
database it was aimed at. Stop the instance and use PostgreSQL:

```bash
sudo systemctl stop k-vet@staging
sudo -u postgres dropdb kvet_staging
sudo -u postgres createdb -O kvet_staging kvet_staging --encoding=UTF8
sudo systemctl start k-vet@staging          # migrations run at start, as always
```

The attachments directory is not touched by this, so clear it in the same breath if you want a
clean slate — otherwise its files stay while the rows that pointed at them are gone.

This is a commissioning and test-environment tool. **Check the database name twice**, and once the
practice has billed anything, restore from a backup instead — see [Backups](#backups).

## 5. Build and start

Install the unit and the environment file once:

```bash
sudo cp deploy/k-vet@.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo mkdir -p /etc/k-vet
sudo cp deploy/env.example /etc/k-vet/prod.env
sudoedit /etc/k-vet/prod.env          # KVET_VERSION, and the port if this is not prod
```

Build the image and start the instance:

```bash
git checkout v1.0.0                                          # a released tag
docker build --build-arg KVET_VERSION=1.0.0 -t k-vet:1.0.0 . # first build on a Pi: 30–60 min
sudo systemctl enable --now k-vet@prod
systemctl status k-vet@prod
curl -fsS localhost:8080/healthz                             # → ok
```

After the first time, `deploy/promote.sh prod 1.0.1` does all of that — see
[releasing.md](releasing.md).

The first build compiles the whole Rust dependency tree, Typst included; give the Pi swap
headroom (2 GB is comfortable) and let it run. Rebuilds reuse BuildKit's cargo and target caches
and take minutes. Schema migrations run automatically every time the container starts, so there
is no separate migration step.

Logs: `journalctl -u k-vet@prod -f`.

A second instance is the same three commands with `staging` in place of `prod`, a different
`KVET_HTTP_PORT`, and its own `config.toml` pointing at `kvet_staging`. Give it an
`environment_label` so it cannot be mistaken for the real one, and point its `[mail] smtp_host`
somewhere dead — an instance holding copied data and a working mail server will send real
invoices to real customers.

## 6. First login

Open `base_url` in a browser and sign in with the credentials from `[auth]`. Then, in this order:

1. **Einstellungen** — practice name, address (street, postcode, town, country), practice e-mail,
   IBAN, BIC, bank name, VAT ID, logo. These appear on every invoice
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
sudo -u postgres pg_dump --format=custom kvet > kvet-$(date +%F).dump
sudo tar -czf kvet-files-$(date +%F).tar.gz -C /srv/k-vet/prod attachments templates config.toml
```

Restoring: stop the instance (`sudo systemctl stop k-vet@prod`), `pg_restore` into the database,
unpack the files into `/srv/k-vet/prod`, then start it again. Verify a restore before you need
it — open an old invoice's PDF and check a patient file opens.

Back up the practice's instance. A test instance holds nothing worth keeping, and including it
would double the archive for data you are happy to drop.

## Upgrading

Upgrades go through a release, so this has its own guide: **[releasing.md](releasing.md)**. The
short version, on the Pi:

```bash
deploy/promote.sh staging 1.2.3     # try it here first
deploy/promote.sh prod    1.2.3     # then the identical image
```

Migrations run at start and are forward-only, so an older image will not run against a newer
schema. `promote.sh` dumps the production database before touching it, because that dump — not
the previous image — is what a rollback actually needs. Watch `journalctl -u k-vet@prod -f` on
the first start after an upgrade.

`docker image prune` cleans up untagged layers; leave the tags production is on and the one
before it, or there is nothing to go back to.

## Running without Docker

Not supported as a deployment. The binary alone works for development —
`cargo run` in `k-vet-backend/` with `npm run dev` in `k-vet-web/` — but a production install
needs the built interface on disk and the paths above, which is exactly what the image provides.

A development setup still needs a database, and the rules are the same as in
[section 4](#4-the-database): a role that owns a UTF-8 database, no superuser. The quickest one is
`docker compose up -d db` from the repository root — `docker-compose.yml` is the development
database and is not used for deployment — which gives `postgres://kvet:kvet@localhost:5432/kvet`.
