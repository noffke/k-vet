import { defineConfig, devices } from '@playwright/test'
import { TIME_ZONE } from './fixtures/clock'

/**
 * Workers run spec files in parallel, each with a backend and a database of its own
 * (`fixtures/test.ts`). Four suit an 8-core machine, two a CI runner; `E2E_WORKERS=1` runs one at
 * a time, for following a single failure.
 */
const workers = Number(process.env.E2E_WORKERS ?? (process.env.CI ? 2 : 4))

/**
 * Pixel 9a viewport (1080x2424 physical, DPR 2.625). Playwright's device registry
 * has no Pixel 9a entry, so the descriptor is spelled out here.
 */
const pixel9a = {
  ...devices['Pixel 7'],
  // Screen and viewport have to agree: a mismatch makes Chrome's mobile emulation report a
  // taller inner height than Playwright assumes, and clicks on the fixed tab bar miss.
  viewport: { width: 411, height: 923 },
  screen: { width: 411, height: 923 },
  deviceScaleFactor: 2.625,
}

export default defineConfig({
  testDir: './tests',
  outputDir: './test-results',
  globalSetup: './global-setup.ts',
  // Files are what is spread across workers; a file's tests stay in order, in one worker.
  fullyParallel: false,
  workers,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    // `baseURL` is the worker's own backend, set by the fixture in `fixtures/test.ts`.
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    locale: 'de-DE',
    timezoneId: TIME_ZONE,
  },
  projects: [
    {
      name: 'desktop',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } },
    },
    {
      name: 'pixel-9a',
      use: pixel9a,
    },
  ],
})
