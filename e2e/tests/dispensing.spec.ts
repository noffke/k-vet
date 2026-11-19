import { expect, test } from '../fixtures/test'
import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
import { pdfText, signIn } from './helpers'

/**
 * A drug position is what was dispensed (migration 0039): the vet picks the drug — offered as
 * its original pack — and types the amount in its unit. Whole packs are billed at the pack price,
 * the rest as a Teilmenge, and the invoice prints the two as two rows (requirements/prices.md).
 *
 * The seeded drug is a 100 ml bottle listed at 10,00 €: 17,61 € gross a bottle, and a 10 ml
 * Teilmenge at 2,38 € — 0,238 € a ml.
 */
test.describe('dispensing a drug', () => {
  test('150 ml are a bottle and a Teilmenge, on the line and on the invoice', async ({
    page,
    request,
  }) => {
    const { drugName } = await seedDrug(request)
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(drugName)
    // Only the bottle is offered, not its Teilmenge.
    await expect(page.getByRole('option', { name: new RegExp(drugName) })).toHaveCount(1)
    await page.getByRole('option', { name: /· 100 ml/ }).first().click()

    const quantity = page.getByRole('textbox', { name: 'Menge' }).first()
    await expect(quantity).toHaveValue('10')
    await expect(page.getByText('in Schritten von 10 ml').first()).toBeVisible()
    await quantity.fill('150')
    await quantity.blur()
    await expect(
      page.getByText(/1 × 100 ml à 17,61.€ \+ 50 ml Teilmenge à 0,238.€\/ml/).first(),
    ).toBeVisible()
    // 17,61 € + 11,90 €.
    await expect(page.getByText('29,51 €').first()).toBeVisible()

    // Off the Teilmenge's steps: refused, saying which steps, and the field keeps 150.
    await quantity.fill('155')
    await quantity.blur()
    await expect(page.getByText('Diese Menge geht nicht: in Schritten von 10 ml.')).toBeVisible()
    await expect(quantity).toHaveValue('150')

    // The invoice prints the line as two rows, each with its own price.
    const invoice = await (
      await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })
    ).json()
    const response = await page.request.get(`/api/invoices/${invoice.id}/pdf`)
    expect(response.status()).toBe(200)
    const text = await pdfText(Buffer.from(await response.body()))
    expect(text).toContain('1 Originalpackung à 100 ml')
    expect(text).toContain('Teilmenge 50 ml, mit Anbruchaufschlag')
    expect(text).toContain('0,238 €')
    expect(text).toContain('29,51 €')
  })

  test('a drug marked "Beliebig teilbar" takes any amount', async ({ page, request }) => {
    const { drugId, drugName } = await seedDrug(request)
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)
    // It has its 10 ml Teilmenge: the mark waits until that is deleted (asked first).
    const flag = page.getByLabel('Beliebig teilbar')
    await expect(flag).toBeDisabled()
    await expect(page.getByText(/Erst die Teilmengen unten löschen/)).toBeVisible()
    await page.getByRole('button', { name: 'Teilmenge löschen' }).click()
    await expect(page.getByRole('dialog')).toContainText('Teilmenge 10 ml löschen?')
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen' }).click()
    await expect(page.getByRole('button', { name: 'Teilmenge löschen' })).toHaveCount(0)

    await flag.check()
    await expect(page.getByText(/jede Menge abrechenbar/)).toBeVisible()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    // Marked, it takes no Teilmenge.
    await expect(page.getByRole('button', { name: 'Teilmenge', exact: true })).toBeDisabled()

    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(drugName)
    await page.getByRole('option', { name: /· 100 ml/ }).first().click()
    // One ml to start with, and any amount from there.
    const quantity = page.getByRole('textbox', { name: 'Menge' }).first()
    await expect(quantity).toHaveValue('1')
    await expect(page.getByText('beliebige Menge').first()).toBeVisible()
    await quantity.fill('2,7')
    await quantity.blur()
    await expect(page.getByText(/2,7 ml Teilmenge à 0,238.€\/ml/).first()).toBeVisible()
    // The whole bottle priced as a Teilmenge, shared out: 2,7 × 0,20 € = 0,54 € net, 0,64 € with
    // VAT.
    await expect(page.getByText('0,64 €').first()).toBeVisible()
  })
})
