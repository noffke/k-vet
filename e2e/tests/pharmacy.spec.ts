import { expect, test } from '@playwright/test'
import { germanDate } from '../fixtures/clock'
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
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,80')

    // A 10 ml subset takes its price from § 4: basis 1.00 → 2.00 net → 2.38 gross.
    await page.getByRole('button', { name: 'Teilmenge' }).click()
    // Wait for the row before reaching for `.last()`. The packaging is created on the server,
    // and until its refetch lands `.last()` is still the *original* — `fill` does not retry
    // itself onto the right element, so it would quietly rewrite the original's 100 ml to 10
    // and leave the subset with no quantity, hence no computed price for the assertion below.
    await expect(page.getByLabel('Menge')).toHaveCount(2)
    const subsetUnit = page.getByLabel('Einheit').last()
    await subsetUnit.fill('ml')
    await page.getByLabel('Menge').last().fill('10')
    await expect(page.getByLabel('Verkaufspreis (netto)').last()).toHaveValue('2,00')
    await expect(page.getByLabel('Listenpreis (netto)').last()).toHaveValue('1,00')
  })

  test('releasing an invoice with a euthanasia drug enters the date of death', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const name = `Minka-${Date.now() % 1e6}`
    const patientId = await seedPatient(request, customerId, name)
    const { treatmentId } = await seedTreatment(request, patientId)
    const drug = await seedDrug(request)

    // The mark is set on the drug's page, and says what it does.
    await signIn(page)
    await page.goto(`/pharmacy/${drug.drugId}`)
    await page.getByLabel('Euthanasie').check()
    await expect(page.getByText(/trägt die Freigabe der Rechnung den Tag des Termins/)).toBeVisible()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: { kind: 'drug_packaging', drug_packaging_id: drug.subsetPackagingId, quantity: '1' },
    })
    await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })

    // The release page says it before the button does it.
    const today = germanDate()
    await page.goto(`/treatments/${treatmentId}/invoice/accept`)
    await expect(
      page.getByText(`Mit der Freigabe wird für ${name} das Todesdatum ${today} eingetragen.`),
    ).toBeVisible()
    await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
    await expect(page.getByText('Freigegeben').first()).toBeVisible()

    await page.goto(`/patients/${patientId}`)
    await expect(page.getByLabel('Todesdatum')).toHaveValue(today)
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
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,80')

    // § 3 Abs. 1 Satz 2 instead: 3 % + 8,10 € → 18,40 € net. The checkbox label also proves the
    // i18n key resolves — a missing one would render as `pharmacy.flags.humanDrug`.
    await page.getByLabel('Humanpräparat').check()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('18,40')

    // And back again, so the flag is not a one-way door.
    await page.getByLabel('Humanpräparat').uncheck()
    await expect(page.getByLabel('Verkaufspreis (netto)').first()).toHaveValue('14,80')
  })

  test('a Teilmenge starts on the original packaging unit', async ({ page, request }) => {
    const drug = await seedDrug(request)
    await signIn(page)
    await page.goto(`/pharmacy/${drug.drugId}`)
    await page.getByLabel('Einheit').first().waitFor()

    await page.getByRole('button', { name: 'Teilmenge' }).click()
    // The original is already `ml`, so waiting for the second row is what makes this an
    // assertion about the subset rather than one that passes on the original by coincidence.
    await expect(page.getByLabel('Einheit')).toHaveCount(2)
    // 10 ml out of a 100 ml bottle: the same unit, so it is not retyped every time.
    await expect(page.getByLabel('Einheit').last()).toHaveValue('ml')

    // The field also offers what the practice already writes elsewhere.
    const suggestions = await page
      .locator('#packaging-units option')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('value')))
    expect(suggestions).toContain('ml')
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

  test("an expiry typed as month and year is that month's last day", async ({
    page,
    request,
  }) => {
    const { drugId, packagingId } = await seedDrug(request, { packages: 0 })
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)

    await page.getByRole('button', { name: 'Wareneingang' }).click()
    await page.getByLabel('Anzahl Packungen').fill('1')
    await page.getByLabel('Chargennummer').fill('CH-MONAT')
    const expiry = page.getByLabel('Verfallsdatum')
    await expect(page.getByText('MM.JJJJ für das Monatsende, oder TT.MM.JJJJ')).toBeVisible()
    // As the package prints it; the field shows the date it became.
    await expiry.fill('02.2028')
    await expiry.blur()
    await expect(expiry).toHaveValue('29.02.2028')
    await page.getByRole('button', { name: 'Hinzufügen' }).click()
    await expect(page.getByText('CH-MONAT').filter({ visible: true }).first()).toBeVisible()

    const lots = (await (await request.get(`/api/lots?packaging_id=${packagingId}`)).json()) as {
      batch_number: string
      expiration_date: string
    }[]
    expect(lots.find((lot) => lot.batch_number === 'CH-MONAT')?.expiration_date).toBe('2028-02-29')
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
    // A stocktake is entered on its own page, not in a dialog, and opens with the derived
    // stock filled in (FR-016).
    await expect(page).toHaveURL(/\/lots\/\d+\/correction$/)
    await expect(page.getByLabel('Restbestand').last()).toHaveValue('100')
    await page.getByLabel('Restbestand').last().fill('88')
    await page.getByLabel('Grund').fill('Bruch')
    await page.getByRole('button', { name: 'Bestätigen' }).click()

    await expect(page.getByText('Bruch').filter({ visible: true }).first()).toBeVisible()
    await expect(page.getByText('-12', { exact: false }).first()).toBeVisible()
  })

  test('a stocktake cannot count a lot below zero', async ({ page, request }) => {
    const { drugId, packagingId } = await seedDrug(request)
    await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: `CH-MINUS-${Date.now() % 1e6}` },
    })
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)
    await page.getByText('CH-MINUS-', { exact: false }).filter({ visible: true }).first().click()
    await page.getByRole('button', { name: 'Korrektur' }).click()
    await expect(page).toHaveURL(/\/lots\/\d+\/correction$/)

    // A count of what is on the shelf cannot be below zero (issues.md 8).
    const counted = page.getByLabel('Restbestand').last()
    await counted.fill('-5')
    await page.getByRole('button', { name: 'Bestätigen' }).click()

    // Refused, and said so in its own words rather than the old catch-all "must not be 0".
    await expect(page.getByText('Darf nicht negativ sein')).toBeVisible()
    await expect(page).toHaveURL(/\/lots\/\d+\/correction$/)

    // Emptying it is fine: zero is a stock, below zero is not.
    await counted.fill('0')
    await page.getByLabel('Grund').fill('Aufgebraucht')
    await page.getByRole('button', { name: 'Bestätigen' }).click()
    await expect(page.getByText('Aufgebraucht').filter({ visible: true }).first()).toBeVisible()
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

  test('a drug the shelf cannot cover is marked in the picker and refused with a reason', async ({
    page,
    request,
  }) => {
    // One 100 ml bottle on the shelf.
    const { drugName } = await seedDrug(request, { packages: 1 })
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const refusal =
      'Der Bestand des Medikaments reicht für diese Menge nicht aus (alle Chargen zusammen). ' +
      'Bitte zuerst den Wareneingang oder eine Inventur erfassen.'

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    const picker = page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …')
    const short = 'Bestand reicht nicht für eine Packung'

    await picker.fill(drugName)
    const bottle = page.getByRole('option', { name: /· 100 ml/ }).first()
    await expect(bottle).toBeVisible()
    await expect(bottle.getByLabel(short)).toHaveCount(0)
    await bottle.click()
    await expect(page.getByText(`„${drugName}“ hinzugefügt`)).toBeVisible()

    // The shelf is empty now: said before picking, and the pick is refused with the reason.
    await picker.fill(drugName)
    const subset = page.getByRole('option', { name: /· 10 ml/ }).first()
    await expect(subset.getByLabel(short)).toBeVisible()
    await subset.click()
    await expect(page.getByText(refusal)).toBeVisible()
    await expect(page.getByLabel('Name')).toHaveCount(1)

    // Raising the bottle's quantity is refused alike, and the field shows what was kept.
    await expect(page.getByText(refusal)).toHaveCount(0, { timeout: 10_000 })
    const quantity = page.getByLabel('Menge').first()
    await quantity.fill('2')
    await quantity.blur()
    await expect(page.getByText(refusal)).toBeVisible()
    await expect(quantity).toHaveValue('1')
  })

  test('a lot history links each dispense to patient, customer and invoice', async ({
    page,
    request,
  }) => {
    const { packagingId, subsetPackagingId } = await seedDrug(request)
    // Dated, so FEFO reaches for this lot rather than the undated one the drug was seeded
    // with — undated lots sort last, and the history asserted below is this lot's.
    const intake = await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: 'CH-TRACE', expiration_date: '2026-12-31' },
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
      data: { recipient_emails: [], send_at: new Date().toISOString(), template: 'Standard' },
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
