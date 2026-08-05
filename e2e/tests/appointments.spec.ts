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

    // Icon-only on a phone, but still named.
    await expect(page.getByRole('button', { name: 'Duplizieren', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Löschen', exact: true })).toBeVisible()

    // Still incomplete: a Termin needs a time, and none was invented for it.
    await expect(page.getByText('Unvollständig', { exact: true })).toBeVisible()

    // The time arrives second, and the date typed first is the one that is stored.
    // Typed as digits only: the field is ours now, not the browser's 12-hour control.
    const time = page.getByLabel('Uhrzeit')
    await time.fill('1500')
    await time.blur()
    await expect(time).toHaveValue('15:00')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Unvollständig', { exact: true })).toHaveCount(0)

    await page.reload()
    await expect(page.getByLabel('Datum')).toHaveValue('05.08.2026')
    await expect(page.getByLabel('Uhrzeit')).toHaveValue('15:00')
    await expect(page.getByRole('heading', { name: '05.08.2026' })).toBeVisible()
  })
})

/**
 * The browser's locale, not the app's. `<input type="time">` rendered in this one — an
 * en-US browser turned the German screen's Uhrzeit into a 12-hour AM/PM control that would
 * not take 15:00 — which is why the field is no longer native.
 */
test.describe('appointments on an en-US browser', () => {
  test.use({ locale: 'en-US' })

  test('the time is still German', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByRole('button', { name: 'Neuer Termin' }).click()

    const time = page.getByLabel('Uhrzeit')
    // A native time control's DOM value is `HH:mm` whatever it displays, so that alone
    // proves nothing. These two do: it is not the native control, and it takes an input
    // the native control rejects outright.
    await expect(time).toHaveAttribute('type', 'text')
    await time.fill('1500')
    await time.blur()
    await expect(time).toHaveValue('15:00')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
  })
})
