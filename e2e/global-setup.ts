import { spawn } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Client } from 'pg'
import { E2E_PASSWORD_HASH, E2E_USERNAME } from './fixtures/credentials'
import { startFakeGoogle } from './fixtures/fakeGoogle'

const port = Number(process.env.KVET_PORT ?? 8081)
const baseUrl = process.env.KVET_BASE_URL ?? `http://127.0.0.1:${port}`
const databaseUrl =
  process.env.DATABASE_URL ?? 'postgres://kvet:kvet@localhost:5432/kvet_e2e'
const here = dirname(fileURLToPath(import.meta.url))
const binary =
  process.env.KVET_BINARY ?? resolve(here, '../k-vet-backend/target/debug/k-vet-backend')
// The frontend is served from disk (`server.web_dir`), so the suite needs a built dist.
const webDir = process.env.KVET_WEB_DIR ?? resolve(here, '../k-vet-web/dist')
// The template the container seeds as "Standard".
const seededEmailTemplate = resolve(here, '../k-vet-backend/templates/invoice-email.txt')

function writeConfig(): { configPath: string; attachmentsDir: string; templatesDir: string } {
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
    'Kurz: Rechnung {{ invoice_number }}\n\n{{ greeting }},\nanbei die Rechnung.\n',
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

[[invoice.email_templates]]
name = "Kurz"
file = "invoice-email-kurz.txt"

[[invoice.email_templates]]
name = "Fehlt"
file = "invoice-email-fehlt.txt"

[travel_expenses]
rate_per_double_km = "3.50"
minimum = "13.00"

# Google is the suite's own fake (KVET_MAPS_ENDPOINT); the key only has to be there.
[maps]
api_key = "e2e"
`,
    'utf8',
  )
  return { configPath, attachmentsDir, templatesDir }
}

async function waitForHealth(timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs
  let lastError: unknown
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${baseUrl}/healthz`)
      if (response.ok) return
    } catch (error) {
      lastError = error
    }
    await new Promise((done) => setTimeout(done, 250))
  }
  throw new Error(`k-vet backend did not become healthy at ${baseUrl}: ${String(lastError)}`)
}

export default async function globalSetup(): Promise<() => Promise<void>> {
  if (!existsSync(binary)) {
    throw new Error(
      `backend binary not found at ${binary} — build it first (cargo build) or set KVET_BINARY`,
    )
  }
  if (!existsSync(join(webDir, 'index.html'))) {
    throw new Error(
      `no built frontend at ${webDir} — run \`npm run build\` in k-vet-web/ or set KVET_WEB_DIR`,
    )
  }
  const { configPath, templatesDir } = writeConfig()

  // Deterministic start: empty the database and let the binary's own migrations rebuild it.
  //
  // Done here rather than by the application, which no longer knows how to drop its own
  // schema. An appliance that can do that is a liability once two instances share a machine:
  // the guard could say *that* a reset was allowed but never *which* database it was pointed
  // at. Emptying a database is the database's job, and doing it here also works against a
  // release binary, which a debug-only flag would not have.
  const client = new Client({ connectionString: databaseUrl })
  await client.connect()
  try {
    await client.query('DROP SCHEMA IF EXISTS tower_sessions CASCADE')
    await client.query('DROP SCHEMA IF EXISTS public CASCADE')
    await client.query('CREATE SCHEMA public')
  } finally {
    await client.end()
  }

  const google = await startFakeGoogle()
  const child = spawn(binary, [], {
    // The templates folder is not a config key: the binary reads it from the environment, as
    // in the container image. Nor is where Google is: only a fake is ever put there.
    env: {
      ...process.env,
      KVET_CONFIG: configPath,
      KVET_TEMPLATES_DIR: templatesDir,
      KVET_MAPS_ENDPOINT: google.url,
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  })
  child.on('exit', (code) => {
    if (code !== null && code !== 0) {
      process.stderr.write(`k-vet backend exited with code ${code}\n`)
    }
  })

  await waitForHealth()

  process.env.KVET_E2E_CONFIG = configPath

  return async () => {
    google.server.close()
    child.kill('SIGTERM')
    await new Promise((done) => setTimeout(done, 250))
    if (!child.killed) child.kill('SIGKILL')
  }
}
