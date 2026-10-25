import { expect, type Page, test } from '@playwright/test'
import {
  login,
  seedCustomer,
  seedPatient,
  seedQuote,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Preisauskunft: positions priced for a caller as a treatment prices them, saved as they are
 * put together. With a customer a quote is kept on their page and goes into their visit at the
 * prices quoted; without one it stays open, and leaving it asks whether to keep or discard it.
 */
test.describe('Preisauskunft', () => {
  const total = (page: Page) => page.getByRole('region', { name: 'Gesamt (brutto)' })
  const keepDialog = (page: Page) => page.getByRole('dialog', { name: 'Preisauskunft behalten?' })

  async function pick(page: Page, name: string) {
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(name)
    await page.getByRole('option', { name: new RegExp(name) }).first().click()
  }

  test('prices positions as a treatment would and saves them as typed', async ({
    page,
    request,
  }) => {
    // 23.62 € net, 19 % VAT: 28.11 € gross.
    const { serviceName } = await seedService(request)

    await signIn(page)
    await navigate(page, 'Preisauskunft')
    await pick(page, serviceName)
    await expect(total(page)).toContainText('28,11 €')
    // Saved: the quote has an address of its own now.
    await expect(page).toHaveURL(/\/quote\?id=\d+/)

    await page.getByLabel('Menge').fill('2')
    await expect(total(page)).toContainText('56,22 €')
    await page.getByLabel('Faktor', { exact: true }).fill('200')
    await expect(total(page)).toContainText('112,43 €')
    await expect(total(page)).toContainText('netto 94,48 €')

    await page.reload()
    await expect(total(page)).toContainText('112,43 €')
    await expect(page.getByLabel('Menge')).toHaveValue('2')

    // Leaving an open quote asks; discarding leaves nothing behind.
    await navigate(page, 'Kunden')
    await expect(keepDialog(page)).toBeVisible()
    await keepDialog(page).getByRole('button', { name: 'Verwerfen' }).click()
    await expect(page).toHaveURL(/\/customers/)
    await navigate(page, 'Preisauskunft')
    await expect(page.getByText(serviceName)).toHaveCount(0)
  })

  test('is kept with a new customer made from the caller, and found on their page', async ({
    page,
    request,
  }) => {
    const { serviceName } = await seedService(request)
    const lastName = `Anrufer-${Date.now().toString().slice(-6)}`
    const title = `Impfung ${lastName}`

    await signIn(page)
    await navigate(page, 'Preisauskunft')
    await page.getByLabel('Straße und Hausnummer').fill('Musterweg 5')
    await page.getByLabel('PLZ').fill('12345')
    await page.getByLabel('Ort').fill('Musterstadt')
    await pick(page, serviceName)
    await page.getByLabel('Bezeichnung').fill(title)
    await expect(total(page)).toContainText('28,11 €')

    await navigate(page, 'Termine')
    const dialog = keepDialog(page)
    await expect(dialog).toBeVisible()
    await dialog.getByLabel('Neuer Kunde').check()
    await dialog.getByLabel('Anrede').selectOption('frau')
    await dialog.getByLabel('Vorname').fill('Erika')
    await dialog.getByLabel('Nachname').fill(lastName)
    await expect(dialog).toContainText(
      'Die Adresse Musterweg 5, 12345 Musterstadt wird übernommen.',
    )
    await dialog.getByRole('button', { name: 'Behalten' }).click()
    await expect(page).toHaveURL(/\/appointments/)

    await login(request)
    const found = await (await request.get(`/api/customers?q=${lastName}`)).json()
    const customerId = (found as { id: number }[])[0]?.id
    expect(customerId).toBeTruthy()
    await page.goto(`/customers/${customerId}`)
    await expect(page.getByLabel('Straße und Hausnummer')).toHaveValue('Musterweg 5')
    const listed = page.getByRole('link', { name: new RegExp(title) })
    await expect(listed).toContainText('28,11 €')
    await listed.click()
    await expect(page.getByLabel('Bezeichnung')).toHaveValue(title)
    await expect(page.getByText(serviceName)).toBeVisible()
    // Kept: leaving no longer asks.
    await navigate(page, 'Kunden')
    await expect(keepDialog(page)).toHaveCount(0)
    await expect(page).toHaveURL(/\/customers/)
  })

  test('goes into the customer’s visit at the prices quoted, the change since shown', async ({
    page,
    request,
  }) => {
    const { serviceId, serviceName } = await seedService(request)
    const { customerId } = await seedCustomer(request)
    const title = `Untersuchung ${Date.now().toString().slice(-6)}`
    await seedQuote(request, customerId, title, [{ kind: 'service', id: serviceId, quantity: '1' }])
    // Since the call the price went up: 25.00 € net, 29.75 € gross.
    await request.patch(`/api/services/${serviceId}`, { data: { net_price: '25.00' } })
    const { treatmentId } = await seedTreatment(request, await seedPatient(request, customerId))

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByRole('button', { name: 'Aus Preisauskunft übernehmen' }).click()
    const dialog = page.getByRole('dialog', { name: 'Preisauskunft übernehmen' })
    await dialog.getByRole('button', { name: new RegExp(title) }).click()
    await expect(dialog).toContainText('Preis geändert: heute 29,75 € statt 28,11 €.')
    await expect(dialog.getByLabel('wie angeboten (28,11 €)')).toBeChecked()
    await dialog.getByRole('button', { name: 'Übernehmen' }).click()

    await expect(page.getByText(serviceName).first()).toBeVisible()
    await expect(page.getByLabel('Preis (brutto)').first()).toHaveValue('28,11')
    await page.getByRole('button', { name: 'Rückgängig' }).click()
    await expect(page.getByLabel('Preis (brutto)')).toHaveCount(0)
  })

  test('a quote left open stays reachable, and leaving it asks again', async ({
    page,
    request,
  }) => {
    const { serviceName } = await seedService(request)

    await signIn(page)
    await navigate(page, 'Preisauskunft')
    await pick(page, serviceName)
    await expect(total(page)).toContainText('28,11 €')
    await navigate(page, 'Rechnungen')
    await keepDialog(page).getByRole('button', { name: 'Abbrechen' }).click()
    await expect(page.getByText(serviceName).first()).toBeVisible()

    // The tab goes away without an answer.
    await page.goto('/')
    await navigate(page, 'Preisauskunft')
    const open = page.getByRole('link', { name: new RegExp(serviceName) })
    await expect(open).toContainText('wird am')
    await open.click()
    await expect(total(page)).toContainText('28,11 €')
    await navigate(page, 'Übersicht')
    await expect(keepDialog(page)).toBeVisible()
    await keepDialog(page).getByRole('button', { name: 'Verwerfen' }).click()
    await expect(page).toHaveURL(/\/$/)
  })
})
