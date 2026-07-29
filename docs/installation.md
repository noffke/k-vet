# Installing k-vet

k-vet is a single binary plus a PostgreSQL database. Nothing else is needed at run time: the web
interface is embedded in the binary, and PDFs and emails are produced in-process.

This guide assumes a generic Linux host with systemd and PostgreSQL 16 or newer. Commands are
written for a Debian-family distribution; adapt the package manager to yours.

## 1. Get the binary

Release builds are published for `x86_64` and `aarch64`. Pick the one matching `uname -m`:

```bash
sudo install -m 755 k-vet-backend /usr/local/bin/k-vet-backend
k-vet-backend --help
```

To build it yourself you need the Rust stable toolchain and Node.js (`^20.19 || >=22.12`):

```bash
cd k-vet-web && npm ci && npm run build        # produces k-vet-web/dist
cd ../k-vet-backend
cargo build --release --features embed-frontend
# → target/release/k-vet-backend
```

Cross-compiling for a 64-bit ARM board (Raspberry Pi 4/5) from an x86_64 machine, using
[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```bash
rustup target add aarch64-unknown-linux-gnu
cargo zigbuild --release --features embed-frontend --target aarch64-unknown-linux-gnu
```

Without `--features embed-frontend` the binary serves the API only and answers every other path
with a note saying so — useful for development, wrong for a deployment.

## 2. PostgreSQL

Create a database and a role that owns it. The application applies its own migrations on every
start, so the role needs full rights on its own database and nothing beyond it:

```bash
sudo -u postgres createuser --pwprompt kvet
sudo -u postgres createdb --owner=kvet kvet
```

Nothing else has to be prepared: extensions (`pg_trgm`), tables, views, indexes and the GOT 2022
fee schedule are all created by the migrations at first start.

## 3. System user and directories

```bash
sudo useradd --system --home /var/lib/k-vet --shell /usr/sbin/nologin kvet
sudo install -d -o kvet -g kvet -m 750 /var/lib/k-vet /var/lib/k-vet/attachments
sudo install -d -o root -g kvet -m 750 /etc/k-vet
```

Attachments (patient files, invoice PDFs, the practice logo) are stored as files under
`attachments_dir`, named by content hash, with their metadata in the database. The two belong
together — see *Backups* below.

## 4. Configuration

Copy [`config.example.toml`](../config.example.toml) to `/etc/k-vet/config.toml`. Every option is
commented there in German and English; this is the walkthrough of what has to be decided.

```bash
sudo cp config.example.toml /etc/k-vet/config.toml
sudo chown root:kvet /etc/k-vet/config.toml && sudo chmod 640 /etc/k-vet/config.toml
```

| Section | Key | What to set |
| --- | --- | --- |
| `[server]` | `listen_address`, `port` | `127.0.0.1:8080` behind a reverse proxy; a LAN address for direct access. |
| | `base_url` | The URL the vet types. An `https://` value also switches the session cookie to `Secure`. |
| | `log_level` | `info` is right; `debug` adds request and SQL detail. |
| `[auth]` | `username` | The single login name. |
| | `password_hash` | Generate with `k-vet-backend --hash-password`: it prompts on the terminal, reads the password from stdin and prints an argon2id hash. Never store the plain password. |
| `[database]` | `url` | `postgres://kvet:PASSWORD@localhost:5432/kvet` |
| | `max_connections` | 5 is plenty for one user; raise only with a reason. |
| `[storage]` | `attachments_dir` | `/var/lib/k-vet/attachments` |
| | `max_upload_mb` | Rejects larger uploads with 413. |
| `[mail]` | `smtp_host`, `smtp_port`, `smtp_tls` | The practice's own mail server. `smtp_tls` is `starttls`, `tls` or `none`. |
| | `smtp_username`, `smtp_password` | Credentials; the file is `chmod 640` for a reason. |
| | `from_address`, `from_name` | Sender of invoice emails. |
| `[invoice]` | `number_pattern` | e.g. `{year}-{counter:4}` → `2026-0042`. The counter's scope follows the date placeholders: with `{year}` it restarts yearly, with `{year}`+`{month}` monthly, without any date part it runs forever. It never decrements and a number is never reissued. |
| | `currency`, `vat_rates` | `EUR` and the rates offered for new records. |
| | `typst_template`, `email_template` | Leave empty to use the embedded templates, or point at your own — see [templates.md](templates.md). |
| `[travel_expenses]` | `rate_per_double_km`, `minimum` | GOT § 10 values: `3.50` per double kilometre, `13.00` minimum. Update when the fee schedule changes. |

Practice name, address, IBAN, VAT ID, logo and the global CC/BCC addresses are **not** in this
file — the vet edits them in the application under *Einstellungen*.

## 5. systemd unit

`/etc/systemd/system/k-vet.service`:

```ini
[Unit]
Description=k-vet practice management
After=network-online.target postgresql.service
Wants=network-online.target

[Service]
Type=exec
User=kvet
Group=kvet
Environment=KVET_CONFIG=/etc/k-vet/config.toml
ExecStart=/usr/local/bin/k-vet-backend
Restart=on-failure
RestartSec=5s

# The service needs nothing but its own data directory.
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/k-vet

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now k-vet
systemctl status k-vet
curl -s localhost:8080/healthz     # → ok
```

The configuration file is found through the first command line argument, then `KVET_CONFIG`, then
`/etc/k-vet/config.toml`.

## 6. First login

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

- `GET /healthz` — liveness, no session required.
- `GET /metrics` — Prometheus text format: request counts and latency histograms labelled by
  route pattern. Also unauthenticated, so keep the port off the open internet or restrict
  `/metrics` in the reverse proxy.

Nightly at 03:00 the service refreshes the picker's usage weights and deletes abandoned drafts
(opened, never filled in, older than a day). On the first of each month it also removes uploaded
files nothing references any more.

## Backups

**Back up the database dump and the attachments directory together, from the same point in
time.** Either one alone is incomplete: the database holds the metadata and every invoice's
content, the directory holds the bytes of the PDFs and patient files they point at.

```bash
sudo -u postgres pg_dump --format=custom kvet > kvet-$(date +%F).dump
sudo tar -C /var/lib/k-vet -czf kvet-attachments-$(date +%F).tar.gz attachments
```

Restoring is the reverse: create an empty database, `pg_restore` into it, unpack the attachments
directory, start the service. Verify a restore before you need it — open an old invoice's PDF and
check a patient file opens.

## Upgrading

```bash
sudo systemctl stop k-vet
sudo install -m 755 k-vet-backend /usr/local/bin/k-vet-backend
sudo systemctl start k-vet
```

Migrations run automatically at start and are forward-only; take a backup first (see above) and
watch `journalctl -u k-vet -f` on the first start after an upgrade.
