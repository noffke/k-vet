import { expect, test } from '@playwright/test'
import { seedCustomer, seedPatient, seedService, seedTreatment } from '../fixtures/seed'
import { pdfText, signIn } from './helpers'

/**
 * What the rendered invoice says and in which order (issues.md 12, 16, 17).
 *
 * These are claims about the PDF the customer receives, so they are checked against the real
 * document rather than against the data that went into it.
 */
/** Distinctive enough that finding it in the PDF could only mean the note was printed. */
const INTERNAL_NOTE = 'Interner Vermerk: Halterin telefonisch erinnern'

test.describe('the invoice document', () => {
  test('prices the unit with the factor, and closes with the payment details', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)

    // The GiroCode block only renders when the practice has bank details, and this spec asserts
    // where that block sits — so it puts them there itself rather than relying on whichever
    // other spec ran first (the specs share one database). `seedCustomer` has logged in by now.
    const settings = await request.patch('/api/settings', {
      data: { iban: `DE${Date.now().toString().slice(-18)}`, bic: 'BYLADEM1001' },
    })
    expect(settings.status()).toBe(200)

    const patientId = await seedPatient(request, customerId, `Bello-${Date.now() % 1e6}`)
    const { treatmentId } = await seedTreatment(request, patientId)
    // 23.62 net at the 1.5-fold rate: 35.43 net → 42.16 gross per unit at 19 %.
    const { serviceName } = await seedService(request)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)

    const picker = page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …')
    await picker.fill(serviceName)
    await page.getByRole('option', { name: new RegExp(serviceName) }).first().click()

    // The pick has to have landed first, or the id read back here is `undefined`.
    await expect(page.getByLabel('Name')).toHaveCount(1)
    const items = await (await request.get(`/api/treatments/${treatmentId}/items`)).json()
    await request.patch(`/api/treatment-items/${items[0].id}`, { data: { factor: '150.000' } })

    // Record something under both headings, so the report block is printed. `patients[].id` is
    // the Patientenbehandlung, which is what carries the two texts.
    const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()
    const recorded = await request.patch(
      `/api/patient-treatments/${treatment.patients[0].id}`,
      { data: { treatment_reason: 'Routinekontrolle', finding: 'Kontrolle in vier Wochen' } },
    )
    expect(recorded.status()).toBe(200)

    await page.reload()
    await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
    await expect(page).toHaveURL(/\/treatments\/\d+\/invoice$/)
    await expect(page.getByLabel('Therapie und weiteres Vorgehen aufführen')).toBeChecked()
    // Written for the practice, not the customer — it must not reach the document.
    await page.getByLabel('Notiz (intern)').fill(INTERNAL_NOTE)
    const openedTab = page.context().waitForEvent('page')
    await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
    await (await openedTab).close()
    await expect(page).toHaveURL(/\/treatments\/\d+$/)

    const created = await (await request.get(`/api/treatments/${treatmentId}`)).json()
    const pdf = await page.request.get(`/api/invoices/${created.invoice.id}/pdf`)
    expect(pdf.status()).toBe(200)
    const text = await pdfText(Buffer.from(await pdf.body()))

    // issues.md 16: the Einzelpreis carries the Steigerungssatz, so it multiplies out to the
    // line total instead of reading 28,11 € against a 42,16 € line.
    expect(text).not.toContain('28,11 €')
    expect(text).toContain('42,16 €')
    // The Steigerungssatz is still stated — GOT requires it, and it explains the price.
    expect(text).toContain('Faktor: 150 %')

    // The internal note stays internal.
    expect(text).not.toContain(INTERNAL_NOTE)

    // issues.md 12: the two report headings are the vet's words, not the old ones.
    expect(text).toContain('Vorbericht und Untersuchung:')
    expect(text).toContain('Therapie und weiteres Vorgehen:')
    expect(text).not.toContain('Behandlungsgrund:')
    expect(text).not.toContain('Befund:')

    // issues.md 17: the payment details come last — after the report the customer reads first.
    expect(text.indexOf('GiroCode')).toBeGreaterThan(text.indexOf('Vorbericht und Untersuchung:'))
  })
})
