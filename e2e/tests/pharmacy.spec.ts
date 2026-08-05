import { expect, test } from '@playwright/test'
import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Pharmacy (quickstart #7 and #4): a delivery becomes stock, a treatment dispenses it
 * FEFO across lots, accepting the invoice freezes it, cancelling gives it back — and the
 * lot's history says who received what.
 */
test.describe('pharmacy', () => {
  test('a drug is created with a computed AMPreisV price and a subset', async ({
    page,
    request,
  }) => {
    // Seeding a drug also creates the supplier and manufacturer the form picks from.
    await seedDrug(request)
    await signIn(page)
    await navigate(page, 'Apotheke')
    await page.getByRole('button', { name: 'Neues Medikament' }).click()

    await page.getByLabel('Name').fill('Testarznei')
    await page.getByLabel('Hersteller').selectOption({ index: 1 })
    await page.getByLabel('USt.').selectOption('19.000')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // The original packaging: 100 ml bought for 10.00 EUR net.
    await page.getByRole('button', { name: 'Originalpackung' }).click()
    await page.getByLabel('Einheit').fill('ml')
    await page.getByLabel('Menge').fill('100')
    await page.getByLabel('Listenpreis (netto)').fill('10,00')
    await page.getByLabel('Lieferant').selectOption({ index: 1 })

    // § 3(3) band 8.68–12.14 → 48 %: 10.00 + 4.80 = 14.80 net (17,61 € gross).
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,8')

    // A 10 ml subset takes its price from § 4: basis 1.00 → 2.00 net → 2.38 gross.
    await page.getByRole('button', { name: 'Teilmenge' }).click()
    const subsetUnit = page.getByLabel('Einheit').last()
    await subsetUnit.fill('ml')
    await page.getByLabel('Menge').last().fill('10')
    await expect(page.getByLabel('Verkaufspreis (netto)').last()).toHaveValue('2')
    await expect(page.getByLabel('Listenpreis (netto)').last()).toHaveValue('1')
  })

  test('marking a drug as a Humanpräparat switches the AMPreisV rule', async ({
    page,
    request,
  }) => {
    await seedDrug(request)
    await signIn(page)
    await navigate(page, 'Apotheke')
    await page.getByRole('button', { name: 'Neues Medikament' }).click()

    await page.getByLabel('Name').fill('Humanpräparat-Test')
    await page.getByLabel('Hersteller').selectOption({ index: 1 })
    await page.getByLabel('USt.').selectOption('19.000')

    await page.getByRole('button', { name: 'Originalpackung' }).click()
    await page.getByLabel('Einheit').fill('ml')
    await page.getByLabel('Menge').fill('100')
    await page.getByLabel('Listenpreis (netto)').fill('10,00')
    await page.getByLabel('Lieferant').selectOption({ index: 1 })

    // The veterinary bands of § 3 Abs. 3: 48 % of 10,00 € → 14,80 € net.
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,8')

    // § 3 Abs. 1 Satz 2 instead: 3 % + 8,10 € → 18,40 € net. The checkbox label also proves the
    // i18n key resolves — a missing one would render as `pharmacy.flags.humanDrug`.
    await page.getByLabel('Humanpräparat').check()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('18,4')

    // And back again, so the flag is not a one-way door.
    await page.getByLabel('Humanpräparat').uncheck()
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,8')
  })

  test('a delivery becomes a lot whose stock is derived', async ({ page, request }) => {
    const { drugId } = await seedDrug(request)
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)

    await page.getByRole('button', { name: 'Wareneingang' }).click()
    await page.getByLabel('Anzahl Packungen').fill('2')
    await page.getByLabel('Chargennummer').fill('CH-2026-07')
    await page.getByLabel('Verfallsdatum').fill('31.03.2027')
    await page.getByRole('button', { name: 'Hinzufügen' }).click()

    // 2 packages × 100 ml.
    await expect(page.getByText('CH-2026-07').filter({ visible: true }).first()).toBeVisible()
    await expect(page.getByRole('heading', { name: /Bestand/ })).toContainText('200')
  })

  test('a stocktake books the difference and keeps the history', async ({ page, request }) => {
    const { drugId, packagingId } = await seedDrug(request)
    await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: 'CH-INVENTUR' },
    })
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)
    await page.getByText('CH-INVENTUR').filter({ visible: true }).first().click()

    await page.getByRole('button', { name: 'Korrektur' }).click()
    // The dialog opens with the derived stock filled in (FR-016).
    await expect(page.getByLabel('Restbestand').last()).toHaveValue('100')
    await page.getByLabel('Restbestand').last().fill('88')
    await page.getByLabel('Grund').fill('Bruch')
    await page.getByRole('button', { name: 'Bestätigen' }).click()

    await expect(page.getByText('Bruch').filter({ visible: true }).first()).toBeVisible()
    await expect(page.getByText('-12', { exact: false }).first()).toBeVisible()
  })

  test('dispensing splits FEFO across lots and cancelling returns the stock', async ({
    page,
    request,
  }) => {
    const { drugId, drugName, packagingId } = await seedDrug(request)
    // Lot A expires first but holds only 20 ml (one package of a 20 ml packaging would be
    // awkward, so the intake is corrected down to 20).
    const lotA = await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: 'CH-A', expiration_date: '2026-06-30' },
    })
    const lotAId = (await lotA.json()).id
    await request.post(`/api/lots/${lotAId}/corrections`, {
      data: { new_remaining: '20', reason: 'Anfangsbestand' },
    })
    await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: 'CH-B', expiration_date: '2027-06-30' },
    })

    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)

    // Three 10 ml subsets = 30 ml: 20 from CH-A, 10 from CH-B.
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()
    await page.getByLabel('Menge').first().fill('3')
    await expect(page.getByText(/CH-A/).first()).toBeVisible()
    await expect(page.getByText(/CH-B/).first()).toBeVisible()

    // Accept the invoice: the dispense is frozen.
    await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
    await page.getByRole('button', { name: 'Rechnung erstellen' }).last().click()
    await page.getByRole('button', { name: 'Rechnung freigeben' }).click()
    await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
    await expect(page.getByText('Freigegeben').first()).toBeVisible()

    await page.goto(`/pharmacy/${drugId}`)
    await expect(page.getByRole('heading', { name: /Bestand/ })).toContainText('90')

    // Cancelling writes the compensating corrections; the shelf is whole again.
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByRole('button', { name: 'Rechnung stornieren' }).click()
    await page.getByRole('button', { name: 'Bestätigen' }).click()
    await expect(page.getByText('Storniert').first()).toBeVisible()

    await page.goto(`/pharmacy/${drugId}`)
    await expect(page.getByRole('heading', { name: /Bestand/ })).toContainText('120')
  })

  test('a lot history links each dispense to patient, customer and invoice', async ({
    page,
    request,
  }) => {
    const { packagingId, subsetPackagingId } = await seedDrug(request)
    const intake = await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: 'CH-TRACE' },
    })
    const lotId = (await intake.json()).id

    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId, 'Rex')
    const { treatmentId } = await seedTreatment(request, patientId)
    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: {
        kind: 'drug_packaging',
        drug_packaging_id: subsetPackagingId,
        quantity: '1',
      },
    })
    const invoice = await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })
    const invoiceId = (await invoice.json()).id
    await request.post(`/api/invoices/${invoiceId}/accept`, {
      data: { recipient_emails: [] },
    })

    await signIn(page)
    await page.goto(`/lots/${lotId}`)

    // SC-004: batch number → who received it, in one view.
    await expect(page.getByRole('link', { name: 'Rex' })).toBeVisible()
    await expect(page.getByRole('link', { name: /Mustermann/ })).toBeVisible()
    await expect(page.getByRole('link', { name: 'Behandlung', exact: true })).toBeVisible()
    await expect(page.getByText((await invoice.json()).invoice_number)).toBeVisible()
  })
})
