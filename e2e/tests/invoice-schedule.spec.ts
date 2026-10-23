import { expect, test } from '@playwright/test'
import {
  clearPendingInvoices,
  seedCustomer,
  seedPatient,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * "Später versenden" (migration 0027): after a euthanasia the vet releases the invoice the same
 * day — documented, written, off her mind — but the owners get it the next morning, not in the
 * middle of their loss. Wherever the invoice shows up unsent, it says that it is waiting on
 * purpose, and until when.
 */
test.describe('sending an invoice later', () => {
  test('a release can wait until tomorrow morning, and says so everywhere', async ({
    page,
    request,
  }) => {
    // The dashboard lists the oldest few unsent invoices; start from none.
    await clearPendingInvoices(request)
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const { serviceId } = await seedService(request)
    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: { kind: 'service', service_id: serviceId, quantity: '1' },
    })
    const invoice = (await (
      await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })
    ).json()) as { invoice_number: string }

    const tomorrow = new Date()
    tomorrow.setDate(tomorrow.getDate() + 1)
    const day = tomorrow.toLocaleDateString('de-DE', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
    })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}/invoice/accept`)
    // "Gleich" unless the vet says otherwise; "Später" offers tomorrow at eight.
    await expect(page.getByLabel('Gleich versenden')).toBeChecked()
    await page.getByLabel('Später versenden').check()
    await expect(page.getByLabel('Datum', { exact: true })).toHaveValue(day)
    await expect(page.getByLabel('Uhrzeit')).toHaveValue('8')
    await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
    await expect(page.getByText(`Die E-Mail wird am ${day} um 08:00 versendet.`)).toBeVisible()

    // The appointment says when.
    await expect(
      page.getByText(`E-Mail an erika@example.com geplant für ${day}, 08:00.`),
    ).toBeVisible()

    // So does the invoices list, where the invoice would otherwise look forgotten…
    await navigate(page, 'Rechnungen')
    await page.getByPlaceholder('Rechnungsnummer oder Kunde suchen …').fill(invoice.invoice_number)
    await expect(
      page.getByText(`E-Mail geplant: ${day}, 08:00`).filter({ visible: true }).first(),
    ).toBeVisible()

    // …and the dashboard's "Nicht versendet".
    await navigate(page, 'Übersicht')
    const unsent = page.locator('section').filter({ hasText: 'Nicht versendet' }).first()
    await expect(unsent.getByText(`E-Mail geplant: ${day}, 08:00`)).toBeVisible()

    // "E-Mail senden" reopens on the schedule, so changing an address does not send it now by
    // accident — choosing "Gleich" does.
    await page.goto(`/treatments/${treatmentId}/invoice/send`)
    await expect(page.getByLabel('Später versenden')).toBeChecked()
    await expect(page.getByLabel('Datum', { exact: true })).toHaveValue(day)
    await page.getByLabel('Gleich versenden').check()
    await page.getByRole('button', { name: 'E-Mail senden', exact: true }).click()
    await expect(
      page.getByText(/E-Mail an erika@example\.com wartet auf den Versand/),
    ).toBeVisible()
  })

  test('a time already past is caught before releasing', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const { serviceId } = await seedService(request)
    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: { kind: 'service', service_id: serviceId, quantity: '1' },
    })
    await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}/invoice/accept`)
    await page.getByLabel('Später versenden').check()
    await page.getByLabel('Datum', { exact: true }).fill('01.01.2020')
    await page.getByLabel('Datum', { exact: true }).blur()
    await expect(page.getByText('Dieser Zeitpunkt ist schon vorbei.')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Rechnung freigeben' }).last()).toBeDisabled()
  })
})
