import { expect, type Page, test } from '@playwright/test'
import { MISSPELLED_STREET, ROUTED_KM, ROUTED_STREET } from '../fixtures/fakeGoogle'
import { seedCustomer, seedPracticeAddress, seedService } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Preisauskunft: a caller asks what a treatment would cost. Positions are priced by the server
 * as a treatment's are, the total is what the vet tells the caller, and nothing is saved — the
 * quote lives in the tab and survives a reload.
 */
test.describe('Preisauskunft', () => {
  const total = (page: Page) => page.getByRole('region', { name: 'Gesamt (brutto)' })

  async function pick(page: Page, name: string) {
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(name)
    await page.getByRole('option', { name: new RegExp(name) }).first().click()
  }

  test('prices positions as a treatment would, and keeps them across a reload', async ({
    page,
    request,
  }) => {
    // 23.62 € net, 19 % VAT: 28.11 € gross.
    const { serviceName } = await seedService(request)

    await signIn(page)
    await navigate(page, 'Preisauskunft')
    await expect(page.getByText('Noch keine Positionen.', { exact: false })).toBeVisible()

    await pick(page, serviceName)
    await expect(total(page)).toContainText('28,11 €')

    // Two of them: 47.24 net + 8.98 VAT.
    await page.getByLabel('Menge').fill('2')
    await expect(total(page)).toContainText('56,22 €')
    // At twice the rate: 94.48 net + 17.95 VAT.
    await page.getByLabel('Faktor', { exact: true }).fill('200')
    await expect(total(page)).toContainText('112,43 €')
    await expect(total(page)).toContainText('netto 94,48 €')

    await page.reload()
    await expect(total(page)).toContainText('112,43 €')
    await expect(page.getByLabel('Menge')).toHaveValue('2')

    await page.getByRole('button', { name: 'Neue Preisauskunft' }).click()
    await expect(page.getByText('Noch keine Positionen.', { exact: false })).toBeVisible()
    await expect(total(page)).toHaveCount(0)
  })

  test('works out the Wegegeld for a customer and for a new caller', async ({ page, request }) => {
    await seedPracticeAddress(request)
    const { serviceName } = await seedService(request, {
      name: 'Wegegeld',
      netPrice: '13.00',
      travelExpenses: true,
    })
    const { lastName } = await seedCustomer(request, { street: `${ROUTED_STREET} 1` })

    await signIn(page)
    await navigate(page, 'Preisauskunft')

    // An existing customer: the drive goes to their home.
    await page.getByPlaceholder('Kunde suchen …').fill(lastName)
    await page.getByRole('option', { name: new RegExp(lastName) }).first().click()
    await expect(
      page.getByRole('link', { name: `${ROUTED_STREET} 1, 12345 Musterstadt` }),
    ).toBeVisible()
    await pick(page, serviceName)
    await expect(page.getByLabel('Kilometer')).toHaveValue(ROUTED_KM)
    // 12 km × 3.50 € = 42.00 € net, 49.98 € gross.
    await expect(total(page)).toContainText('49,98 €')

    // A new caller: an address instead — checked, and asked for its kilometres.
    await page.getByRole('button', { name: 'Neue Preisauskunft' }).click()
    await pick(page, serviceName)
    await expect(page.getByLabel('Kilometer')).toHaveValue('')
    await expect(page.getByText('Ohne Kilometer gilt der Katalogpreis.')).toBeVisible()
    await expect(total(page)).toContainText('15,47 €')

    const street = page.getByLabel('Straße und Hausnummer')
    await street.fill(`${MISSPELLED_STREET} 5`)
    await page.getByLabel('PLZ').fill('12345')
    await page.getByLabel('Ort').fill('Musterstadt')
    await page.getByRole('heading', { name: 'Preisauskunft' }).click()
    await expect(page.getByText('Meinten Sie: Musterweg 5, 12345 Musterstadt?')).toBeVisible()
    await page.getByRole('button', { name: 'Übernehmen' }).click()
    await expect(street).toHaveValue('Musterweg 5')
    await expect(page.getByText('Adresse gefunden')).toBeVisible()
    // Google knows no route to Musterweg: no kilometres, and it says so.
    await expect(page.getByText(/Entfernung nicht ermittelt/).first()).toBeVisible()
    await expect(page.getByLabel('Kilometer')).toHaveValue('')

    await street.fill(`${ROUTED_STREET} 3`)
    await page.getByRole('heading', { name: 'Preisauskunft' }).click()
    await expect(page.getByLabel('Kilometer')).toHaveValue(ROUTED_KM)
    await expect(total(page)).toContainText('49,98 €')
  })
})
