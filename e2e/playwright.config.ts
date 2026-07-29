import { defineConfig, devices } from '@playwright/test'

const port = Number(process.env.KVET_PORT ?? 8081)

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
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: process.env.KVET_BASE_URL ?? `http://127.0.0.1:${port}`,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    locale: 'de-DE',
    timezoneId: 'Europe/Berlin',
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
