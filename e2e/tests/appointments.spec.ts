import { expect, test } from '@playwright/test'
import { seedAcceptedInvoice } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Entering a Termin (issues.md): the date is offered, typed short, and — the part that used
 * to be lost — kept while the appointment is still waiting for its time.
 */
test.describe('appointments', () => {
  test('a Termin is only deleted once, and never after it has been billed', async ({
    page,
    request,
  }) => {
    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByRole('button', { name: 'Neuer Termin' }).click()
    // "Neuer Termin" creates the record and routes to it; read the URL once that has happened,
    // not before, or this captures the list it is still leaving.
    await expect(page).toHaveURL(/\/appointments\/\d+$/)
    const url = page.url()

    // Asked first (issues.md 2): a dismissed dialog leaves the appointment alone.
    await page.getByRole('button', { name: 'Löschen' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Abbrechen' }).click()
    await expect(page).toHaveURL(url)

    await page.getByRole('button', { name: 'Löschen' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen' }).click()
    await expect(page).toHaveURL(/\/appointments$/)

    // Billed, then cancelled. The number is burned either way, so the Termin stays — and the
    // screen has to say so rather than leaving a dead button (issues.md 1).
    const invoice = await seedAcceptedInvoice(request)
    const cancel = await request.post(`/api/invoices/${invoice.invoiceId}/cancel`)
    expect(cancel.ok()).toBeTruthy()

    const appointment = await request.get(`/api/treatments/${invoice.treatmentId}`)
    const appointmentId = ((await appointment.json()) as { appointment_id: number }).appointment_id
    await page.goto(`/appointments/${appointmentId}`)

    await page.getByRole('button', { name: 'Löschen' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen' }).click()
    await expect(page).toHaveURL(new RegExp(`/appointments/${appointmentId}$`))
    await expect(page.getByText(/bereits eine Rechnung geschrieben/)).toBeVisible()
  })

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
