import { expect, test } from '@playwright/test'
import { login, seedCustomer, seedPatient, seedTreatment } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Auto-save is the top UX requirement (Constitution III, SC-002): anything typed and
 * visible must survive navigation and reload without a save action. Quickstart #2.
 */
test.describe('auto-save', () => {
  test('a customer field survives a reload without any save action', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page)

    await page.goto(`/customers/${customerId}`)
    const warning = page.getByLabel('Warnhinweis')
    await warning.fill('Hund beißt bei der Blutabnahme')

    // Wait for the indicator rather than a fixed delay: it is the app's own promise.
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await page.reload()
    await expect(page.getByLabel('Warnhinweis')).toHaveValue('Hund beißt bei der Blutabnahme')
  })

  test('leaving the page mid-typing keeps the last state', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    await signIn(page)

    await page.goto(`/treatments/${treatmentId}`)
    await page.getByLabel('Behandlungsgrund').fill('Impfung und Kontrolle')

    // Navigate away immediately: the pending change is flushed, not dropped.
    await navigate(page, 'Kunden')
    await expect(page).toHaveURL(/\/customers$/)

    await page.goto(`/treatments/${treatmentId}`)
    await expect(page.getByLabel('Behandlungsgrund')).toHaveValue('Impfung und Kontrolle')
  })

  test('an invalid value is not stored and the last valid one stays', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    await login(request)
    await signIn(page)

    await page.goto(`/customers/${customerId}`)
    const phone = page.getByLabel('Telefon')
    await phone.fill('keine Nummer')
    await phone.blur()

    // The field itself explains the problem ...
    await expect(page.getByText('Keine gültige Telefonnummer')).toBeVisible()
    await expect(page.getByLabel('Telefon')).toHaveValue('keine Nummer')

    // ... while the stored record still has the last valid number.
    const stored = await request.get(`/api/customers/${customerId}`)
    expect((await stored.json()).phone).toBe('+493012345678')
  })
})
