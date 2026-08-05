import { expect, test } from '@playwright/test'
import { navigate, signIn } from './helpers'

/**
 * Entering a Termin (issues.md): the date is offered, typed short, and — the part that used
 * to be lost — kept while the appointment is still waiting for its time.
 */
test.describe('appointments', () => {
  test('a date typed before the time survives and is expanded', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByRole('button', { name: 'Neuer Termin' }).click()

    // Short input, no padding and no century — expanded on blur.
    const date = page.getByLabel('Datum')
    await date.fill('5.8.26')
    await date.blur()
    await expect(date).toHaveValue('05.08.2026')

    // Still incomplete: a Termin needs a time, and none was invented for it.
    await expect(page.getByText('Unvollständig', { exact: true })).toBeVisible()

    // The time arrives second, and the date typed first is the one that is stored.
    await page.getByLabel('Uhrzeit').fill('15:00')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Unvollständig', { exact: true })).toHaveCount(0)

    await page.reload()
    await expect(page.getByLabel('Datum')).toHaveValue('05.08.2026')
    await expect(page.getByLabel('Uhrzeit')).toHaveValue('15:00')
    await expect(page.getByRole('heading', { name: '05.08.2026' })).toBeVisible()
  })
})
