import { expect, type Page } from '@playwright/test'
import { E2E_PASSWORD, E2E_USERNAME } from '../fixtures/credentials'

/**
 * A page that scrolls sideways is a layout bug on a phone — and it also shrinks Chrome's
 * page scale, which moves the fixed tab bar out of the viewport. Checked on every
 * navigation so no screen can regress unnoticed (Constitution III).
 */
export async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  expect(overflow, 'the page must fit the viewport width').toBeLessThanOrEqual(0)
}

/**
 * Clicks a navigation entry, whichever chrome the viewport shows: the desktop rail or the
 * phone tab bar (plus its overflow sheet for the secondary sections).
 */
export async function navigate(page: Page, label: string): Promise<void> {
  const link = page.getByRole('link', { name: label }).filter({ visible: true })
  if ((await link.count()) === 0) {
    // Sections that live in the phone overflow sheet.
    await page.getByRole('button', { name: 'Öffnen' }).click()
  }
  await page.getByRole('link', { name: label }).filter({ visible: true }).first().click()
  await expectNoSidewaysScroll(page)
}

/** Signs in through the real login form and waits for the app shell. */
export async function signIn(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByLabel('Benutzername').fill(E2E_USERNAME)
  await page.getByLabel('Passwort').fill(E2E_PASSWORD)
  await page.getByRole('button', { name: 'Anmelden' }).click()
  await page.getByRole('heading', { name: 'Übersicht' }).waitFor()
  await expectNoSidewaysScroll(page)
}
