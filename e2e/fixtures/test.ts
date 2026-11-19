import { test as base } from '@playwright/test'
import { type Backend, startBackend } from './backend'

export type { APIRequestContext, Locator, Page } from '@playwright/test'
export { expect } from '@playwright/test'

/**
 * Playwright's `test`, with a backend per worker: each worker starts its own k-vet process on
 * its own port and database before its first test and keeps it for every test it runs, across
 * spec files and both projects; it goes when the worker does. Playwright replaces a worker after
 * a failed test, and the new one starts on a fresh database — nothing a failure left behind
 * leaks into later specs.
 *
 * Specs import `test` and `expect` from here, not from `@playwright/test`: `baseURL` then points
 * at the worker's backend, and `DATABASE_URL` at its database — every worker is its own process,
 * so the seed helpers that reach Postgres directly follow without being told.
 */
export const test = base.extend<object, { backend: Backend }>({
  backend: [
    // biome-ignore lint/correctness/noEmptyPattern: Playwright reads a fixture's dependencies from this pattern.
    async ({}, use, workerInfo) => {
      const backend = await startBackend(workerInfo.workerIndex, workerInfo.parallelIndex)
      process.env.DATABASE_URL = backend.databaseUrl
      await use(backend)
      await backend.stop()
    },
    { scope: 'worker', auto: true, timeout: 120_000 },
  ],
  baseURL: async ({ backend }, use) => {
    await use(backend.url)
  },
})
