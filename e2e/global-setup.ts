import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { binary, webDir } from './fixtures/backend'
import { startFakeGoogle } from './fixtures/fakeGoogle'

/**
 * What the whole run shares: the checks that the binary and the built frontend are there, and
 * the fake Google every backend asks (stateless, so one serves them all). The backends
 * themselves — one per worker, each on its own database — are started by the worker fixture in
 * `fixtures/test.ts`, and stop with their worker.
 */
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

  // Where Google is, for every worker's backend: the workers inherit this process's environment.
  const google = await startFakeGoogle()
  process.env.KVET_MAPS_ENDPOINT = google.url

  return async () => {
    google.server.close()
  }
}
