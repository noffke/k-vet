import { expect, type Page, test } from '@playwright/test'
import { germanDate } from '../fixtures/clock'
import { ROUTED_KM, ROUTED_STREET } from '../fixtures/fakeGoogle'
import {
  seedCustomer,
  seedPatient,
  seedPracticeAddress,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * A Wegegeld line's kilometres: Google's shortest route the first time, the vet's own figure
 * from the previous visit after that, Google again once the customer has moved — and always the
 * vet's to change. Google is the suite's fake (`fixtures/fakeGoogle.ts`).
 */
test.describe('Wegegeld kilometres', () => {
  const today = germanDate()

  async function pick(page: Page, treatmentId: number, serviceName: string) {
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(serviceName)
    await page.getByRole('option', { name: new RegExp(serviceName) }).first().click()
  }

  test('come from Google, then from the last visit, then from Google after a move', async ({
    page,
    request,
  }) => {
    await seedPracticeAddress(request)
    const { serviceName } = await seedService(request, {
      name: 'Wegegeld',
      netPrice: '13.00',
      travelExpenses: true,
    })
    const { customerId } = await seedCustomer(request, { street: `${ROUTED_STREET} 1` })
    const visit = async () =>
      (await seedTreatment(request, await seedPatient(request, customerId))).treatmentId
    const first = await visit()
    const second = await visit()

    await signIn(page)
    await pick(page, first, serviceName)
    await expect(
      page.getByText(`${serviceName}: ${ROUTED_KM} km, kürzeste Strecke laut Google Maps`),
    ).toBeVisible()
    await expect(page.getByLabel('Kilometer')).toHaveValue(ROUTED_KM)
    // 12 km × 3.50 € = 42.00 € net.
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('49,98')

    // The vet knows a better way; the next visit takes her figure.
    await page.getByLabel('Kilometer').fill('13,2')
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('54,98')
    await pick(page, second, serviceName)
    await expect(
      page.getByText(`${serviceName}: 13,2 km wie beim Termin am ${today}`),
    ).toBeVisible()
    await expect(page.getByLabel('Kilometer')).toHaveValue('13,2')

    // The customer moves: the next visit asks Google again.
    await page.goto(`/customers/${customerId}`)
    await page.getByLabel('Straße und Hausnummer').fill(`${ROUTED_STREET} 7`)
    await page.getByLabel('Telefon').click()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Adresse gefunden')).toBeVisible()
    const third = await visit()
    await pick(page, third, serviceName)
    await expect(
      page.getByText(`${serviceName}: ${ROUTED_KM} km, kürzeste Strecke laut Google Maps`),
    ).toBeVisible()
    await expect(page.getByLabel('Kilometer')).toHaveValue(ROUTED_KM)

    // Typed over, the figure can be fetched again with the icon beside it.
    await page.getByLabel('Kilometer').fill('30')
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('124,95')
    await page.getByRole('button', { name: 'Entfernung neu berechnen (Google Maps)' }).click()
    await expect(page.getByLabel('Kilometer')).toHaveValue(ROUTED_KM)
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('49,98')
  })

  test('hold the invoice back until they are entered', async ({ page, request }) => {
    await seedPracticeAddress(request)
    const { serviceName } = await seedService(request, {
      name: 'Wegegeld',
      netPrice: '13.00',
      travelExpenses: true,
    })
    // Google knows no route to Musterweg: the line stays without kilometres.
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(request, await seedPatient(request, customerId))

    await signIn(page)
    await pick(page, treatmentId, serviceName)
    await expect(page.getByText(`${serviceName}: Entfernung nicht ermittelt.`)).toBeVisible()
    await expect(
      page.getByText('Ohne Kilometer kann keine Rechnung erstellt werden.'),
    ).toBeVisible()

    await page.goto(`/treatments/${treatmentId}/invoice`)
    await expect(
      page.getByText(`${serviceName}: bitte erst die Kilometer eintragen`),
    ).toBeVisible()
    await expect(page.getByRole('button', { name: 'Rechnung erstellen' })).toBeDisabled()

    await page.goto(`/treatments/${treatmentId}`)
    await page.getByLabel('Kilometer').fill('4')
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('16,66')
    await page.goto(`/treatments/${treatmentId}/invoice`)
    await expect(page.getByRole('button', { name: 'Rechnung erstellen' })).toBeEnabled()
  })
})
