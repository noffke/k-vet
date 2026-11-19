import { type ChildProcess, spawn } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Client } from 'pg'
import { TIME_ZONE } from './clock'
import { E2E_PASSWORD_HASH, E2E_USERNAME } from './credentials'

const here = dirname(fileURLToPath(import.meta.url))

/** The binary under test: the debug build, or another one named by `KVET_BINARY`. */
export const binary =
  process.env.KVET_BINARY ?? resolve(here, '../../k-vet-backend/target/debug/k-vet-backend')
/** The frontend is served from disk (`server.web_dir`), so the suite needs a built dist. */
export const webDir = process.env.KVET_WEB_DIR ?? resolve(here, '../../k-vet-web/dist')
// The template the container seeds as "Standard".
const seededEmailTemplate = resolve(here, '../../k-vet-backend/templates/invoice-email.txt')
/**
 * The Postgres server, and the database to connect through: each worker creates its own beside
 * it. Read once, when the module loads — a worker then points `DATABASE_URL` at its own.
 */
const serverUrl = process.env.DATABASE_URL ?? 'postgres://kvet:kvet@localhost:5432/kvet_e2e'
const basePort = Number(process.env.KVET_PORT ?? 8081)

/** One worker's backend: its own process, port and database. */
export interface Backend {
  url: string
  databaseUrl: string
  stop: () => Promise<void>
}

/**
 * Starts a backend for one Playwright worker, on a database of its own created fresh — the
 * binary migrates it at start — so the specs of one worker never see another's data. The port
 * is unique for the worker's lifetime: a worker that replaces a failed one never meets the old
 * one's port.
 */
export async function startBackend(workerIndex: number, parallelIndex: number): Promise<Backend> {
  const port = basePort + 1 + workerIndex
  const url = `http://127.0.0.1:${port}`
  const name = `kvet_e2e_w${parallelIndex}`
  const database = new URL(serverUrl)
  database.pathname = `/${name}`
  const databaseUrl = database.toString()

  const admin = new Client({ connectionString: serverUrl })
  await admin.connect()
  try {
    await admin.query(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`)
    await admin.query(`CREATE DATABASE "${name}"`)
  } finally {
    await admin.end()
  }

  const files = writeConfig(port, url, databaseUrl)
  const child = spawn(binary, [], {
    // The templates folder is not a config key: the binary reads it from the environment, as
    // in the container image. Nor is where Google is: only a fake is ever put there (started
    // once for the run, in global-setup.ts). The time zone is the practice's, as the image
    // sets it, whatever the machine running the suite uses.
    env: {
      ...process.env,
      TZ: TIME_ZONE,
      KVET_CONFIG: files.configPath,
      KVET_TEMPLATES_DIR: files.templatesDir,
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  })
  await waitForHealth(url, child)

  return {
    url,
    databaseUrl,
    stop: async () => {
      await stopProcess(child)
      rmSync(files.dir, { recursive: true, force: true })
    },
  }
}

function writeConfig(
  port: number,
  baseUrl: string,
  databaseUrl: string,
): { dir: string; configPath: string; templatesDir: string } {
  const dir = mkdtempSync(join(tmpdir(), 'kvet-e2e-'))
  const attachmentsDir = join(dir, 'attachments')
  mkdirSync(attachmentsDir, { recursive: true })
  // The templates folder as the container seeds it, plus one template of the suite's own; a
  // third is configured without a file, to be offered disabled.
  const templatesDir = join(dir, 'templates')
  mkdirSync(templatesDir, { recursive: true })
  copyFileSync(seededEmailTemplate, join(templatesDir, 'invoice-email.txt'))
  writeFileSync(
    join(templatesDir, 'invoice-email-kurz.txt'),
    '{{ greeting }},\nanbei die Rechnung.\n',
    'utf8',
  )
  const configPath = join(dir, 'config.toml')
  writeFileSync(
    configPath,
    `[server]
listen_address = "127.0.0.1"
port = ${port}
base_url = "${baseUrl}"
log_level = "info"
web_dir = "${webDir}"

[auth]
username = "${E2E_USERNAME}"
password_hash = "${E2E_PASSWORD_HASH}"

[database]
url = "${databaseUrl}"

[storage]
attachments_dir = "${attachmentsDir}"

[mail]
smtp_host = "127.0.0.1"
smtp_port = 2525
smtp_tls = "none"
from_address = "praxis@example.com"
from_name = "Tierarztpraxis Test"

[invoice]
number_pattern = "{year}-{counter:4}"
currency = "EUR"
vat_rates = ["19.000", "7.000"]

[[invoice.email_templates]]
name = "Standard"
file = "invoice-email.txt"
subject = "Ihre Rechnung {{ invoice_number }} – {{ practice_name }}"

[[invoice.email_templates]]
name = "Kurz"
file = "invoice-email-kurz.txt"
subject = "Kurz: Rechnung {{ invoice_number }}"

[[invoice.email_templates]]
name = "Fehlt"
file = "invoice-email-fehlt.txt"
subject = "Rechnung {{ invoice_number }}"

[travel_expenses]
rate_per_double_km = "3.50"
minimum = "13.00"

# Google is the suite's own fake (KVET_MAPS_ENDPOINT); the key only has to be there.
[maps]
api_key = "e2e"

# The phone's calendar, which the suite reaches with plain requests.
[dav]
enabled = true
`,
    'utf8',
  )
  return { dir, configPath, templatesDir }
}

async function waitForHealth(url: string, child: ChildProcess, timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs
  let lastError: unknown
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`k-vet backend for ${url} exited with code ${child.exitCode}`)
    }
    try {
      const response = await fetch(`${url}/healthz`)
      if (response.ok) return
    } catch (error) {
      lastError = error
    }
    await new Promise((done) => setTimeout(done, 100))
  }
  throw new Error(`k-vet backend did not become healthy at ${url}: ${String(lastError)}`)
}

async function stopProcess(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null) return
  const exited = new Promise<void>((done) => child.once('exit', () => done()))
  child.kill('SIGTERM')
  const timedOut = await Promise.race([
    exited.then(() => false),
    new Promise<boolean>((done) => setTimeout(() => done(true), 5_000)),
  ])
  if (timedOut) child.kill('SIGKILL')
}
