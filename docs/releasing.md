# Releasing k-vet

The practice bills from this application and migrations are forward-only, so there is no undo
for a schema change that turns out wrong. Everything here exists to make sure the version the
vet meets is one that has already been run somewhere else, and that when it does go wrong the
way back is known before it is needed.

Two instances run on the one Pi:

| | `k-vet@prod` | `k-vet@staging` |
| --- | --- | --- |
| port | 8080 | 8081 |
| state | `/srv/k-vet/prod/` | `/srv/k-vet/staging/` |
| database | `kvet` | `kvet_staging` |
| version | `/etc/k-vet/prod.env` | `/etc/k-vet/staging.env` |

**`KVET_VERSION` in those two files is the only thing that decides what each instance runs.**
That is the point of the whole arrangement: production changes version when that line changes,
and never because something else was built on the machine.

## Cutting a release

On the development machine, with `main` green:

```bash
scripts/cut-release.sh 1.2.3
```

It refuses to tag a dirty tree, a branch other than `main`, a `main` that is not level with the
remote, a tag that already exists, or a commit CI has not already passed — then tags and pushes.
CI re-runs every gate on the tagged commit, proves the image builds, and publishes the GitHub
release. **A release that exists is one that was verified**, which is what makes a tag safe to
build from later.

The release notes open with the line that decides what you can do next: whether the release
contains migrations.

## Putting it on the Pi

```bash
deploy/promote.sh staging 1.2.3
```

It builds `k-vet:1.2.3` if that image is not already there, rewrites `KVET_VERSION`, restarts
the instance, waits for `/healthz`, and reports the version the instance *actually* answers with
— read from `/metrics`, not from the tag you typed.

Try the change. Then:

```bash
deploy/promote.sh prod 1.2.3
```

The image is already built, so this does not build again: **production runs the identical image
staging ran**, not a second build of the same source. It dumps the database first, to
`/srv/k-vet/backups/`, and tells you where.

## Rolling back

This is the part worth reading before you need it. Migrations run automatically at start and are
forward-only, so an older image will not run against a newer schema.

- **Release without migrations** — set `KVET_VERSION` back in `/etc/k-vet/prod.env` and
  `sudo systemctl restart k-vet@prod`. That is all.
- **Release with migrations** — the old image cannot start against the new schema. Restore the
  dump `promote.sh` took, *then* go back to the old version:

  ```bash
  sudo systemctl stop k-vet@prod
  sudo -u postgres pg_restore -d kvet --clean /srv/k-vet/backups/kvet-…-before-1.2.3.dump
  sudoedit /etc/k-vet/prod.env        # KVET_VERSION back to the previous release
  sudo systemctl start k-vet@prod
  ```

  Anything entered since the dump is lost. This is why the dump is taken at the moment of
  promotion and not on last night's schedule.

Keep the tagged images production is on and the one before it. `docker image prune` removes
untagged layers and leaves tags alone, but `docker image prune -a` would take the way back with
them.

## Version numbers

Semver, starting at `v1.0.0` when the practice goes live. The version is the git tag and nothing
else — it is deliberately not taken from `Cargo.toml`, because that value is baked into the
committed `openapi.json` and bumping it per release would turn every release into a
codegen-drift commit.

What is running, from anywhere:

```bash
curl -fsS localhost:8080/metrics | grep kvet_build_info
journalctl -u k-vet@prod | grep 'k-vet backend started' | tail -1
```

Both answer with the build that is actually serving, which is not the same claim as the tag in
the environment file.

## Before the first release

- Both instances installed and starting, per [installation.md](installation.md).
- The staging instance has an `environment_label` and a dead `[mail] smtp_host`. An instance
  with real-looking data and a working mail server will send real invoices to real customers.
- A restore has been rehearsed once, from a real dump, with an old invoice's PDF opened
  afterwards. An untested backup is not a rollback plan.
