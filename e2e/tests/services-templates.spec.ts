import { expect, test } from '@playwright/test'
import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Services and templates (T066): the GOT catalog is searchable with surgery hidden, a
 * travel line is priced from the kilometres, and a template applies in order with today's
 * prices.
 */
test.describe('services and templates', () => {
  test('the GOT catalog is searchable and surgical positions stay hidden', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Leistungen')

    // A position from Teil A of the imported schedule.
    await page.getByPlaceholder('GOT-Nummer oder Bezeichnung suchen …').fill('Beratung im')
    await expect(
      page.getByText('Beratung im einzelnen Fall', { exact: false }).filter({ visible: true }).first(),
    ).toBeVisible()

    // A surgical position is out of the way until the toggle asks for it.
    await page.getByPlaceholder('GOT-Nummer oder Bezeichnung suchen …').fill('Nephrotomie')
    await expect(page.getByText('Keine Einträge')).toBeVisible()
    await page.getByLabel('Ausgeblendete anzeigen').check()
    await expect(page.getByText('Nephrotomie').filter({ visible: true }).first()).toBeVisible()
  })

  test('a self-defined travel service prices a line from its kilometres', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await navigate(page, 'Leistungen')
    await page.getByRole('button', { name: 'Neue Leistung' }).click()

    await page.getByLabel('Name').fill('Wegegeld Hausbesuch')
    await page.getByLabel('USt.').selectOption('19.000')
    await page.getByLabel('Preis (netto)').fill('13,00')
    await page.getByLabel('Wegegeld').check()
    await page.getByRole('button', { name: 'Schließen', exact: true }).click()

    // On a treatment the line asks for the distance and prices itself (GOT § 10).
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament oder Leistung suchen …').fill('Wegegeld')
    await page.getByRole('option', { name: /Wegegeld/ }).first().click()

    await page.getByLabel('Kilometer').fill('12')
    // 12 km × 3.50 € net = 42.00 € net (GOT quotes the Wegegeld net).
    await expect(page.getByLabel('Preis (netto)').first()).toHaveValue('42')

    await page.getByLabel('Faktor (Verkehrsverhältnisse)').fill('2')
    await expect(page.getByLabel('Preis (netto)').first()).toHaveValue('84')
  })

  test('a template applies its lines in order with current prices', async ({ page, request }) => {
    const { drugName, subsetPackagingId } = await seedDrug(request)
    const templateName = `Impfung-${Date.now().toString().slice(-6)}`
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await navigate(page, 'Behandlungsgruppen')
    await page.getByRole('button', { name: 'Neue Behandlungsgruppe' }).click()
    await page.getByLabel('Name').fill(templateName)
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Two lines: a GOT position and a drug subset, in that order.
    await page.getByPlaceholder('Medikament oder Leistung suchen …').fill('Beratung im')
    await page.getByRole('option').first().click()
    await page.getByPlaceholder('Medikament oder Leistung suchen …').fill(drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()

    // Reordering moves the drug up. The navigation is a list too, so scope to the lines.
    const lines = page.locator('section li')
    await expect(lines).toHaveCount(2)
    await page.getByRole('button', { name: 'An den Anfang' }).last().click()
    await expect(lines.first()).toContainText(drugName)

    // Applying it appends both lines to the treatment, drug first.
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByRole('button', { name: 'Behandlungsgruppe anwenden' }).click()
    await page.getByRole('button', { name: new RegExp(templateName) }).click()

    await expect(page.getByLabel('Name').first()).toHaveValue(new RegExp(drugName))
    // The 10 ml subset of a 100 ml bottle bought for 10.00 net: § 4 basis 1.00, +100 % = 2.00
    // net — pinned at apply time. The customer pays 2,38 € once VAT is added.
    await expect(page.getByLabel('Preis (netto)').first()).toHaveValue('2')
    expect(subsetPackagingId).toBeGreaterThan(0)
  })
})

test.describe('treatment lines', () => {
  test('a drug line can be marked as an ad-hoc Umwidmung', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const drug = await seedDrug(request)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament oder Leistung suchen …').fill(drug.drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()

    // The price before and after must be identical: an Umwidmung documents, it does not price.
    const price = page.getByLabel('Preis (netto)').first()
    const before = await price.inputValue()

    const umwidmung = page.getByLabel('Umwidmung')
    await expect(umwidmung).not.toBeChecked()
    await umwidmung.check()
    await expect(price).toHaveValue(before)

    // It survives a reload — it is stored on the line, not just ticked in the browser.
    await page.reload()
    await expect(page.getByLabel('Umwidmung')).toBeChecked()
  })
})
