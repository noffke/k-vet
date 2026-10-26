import { expect, test } from '@playwright/test'
import { germanDate } from '../fixtures/clock'
import {
  clearPendingInvoices,
  seedCustomer,
  seedPatient,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * "Später versenden" (migration 0027): the invoice is released the same day, its mail goes out
 * at the time chosen — by default the next morning. Wherever the invoice shows up unsent, it
 * says that it is waiting on purpose, and until when.
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

    const day = germanDate(1)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}/invoice/accept`)
    // "Später" unless the vet says otherwise, offering tomorrow at eight.
    await expect(page.getByLabel('Später versenden')).toBeChecked()
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

  test('cancelling the invoice takes its planned mail with it, on every screen', async ({
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
    ).json()) as { id: number; invoice_number: string }
    const day = germanDate(1)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}/invoice/accept`)
    await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
    const planned = page.getByText(`E-Mail an erika@example.com geplant für ${day}, 08:00.`)
    await expect(planned).toBeVisible()

    // The dashboard has seen it planned — and keeps what it read for a while.
    const plannedEntry = page.getByText(`E-Mail geplant: ${day}, 08:00`).filter({ visible: true })
    await navigate(page, 'Übersicht')
    await expect(plannedEntry).toHaveCount(1)
    await page.goBack()

    await page.getByRole('button', { name: 'Rechnung stornieren' }).click()
    await expect(page.getByText('Die geplante E-Mail wird nicht versendet.')).toBeVisible()
    await page.getByRole('button', { name: 'Bestätigen' }).click()
    await expect(page.getByText('Storniert').first()).toBeVisible()
    await expect(planned).toHaveCount(0)
    expect(
      ((await (await request.get(`/api/invoices/${invoice.id}`)).json()) as { email_queue: unknown })
        .email_queue,
    ).toBeNull()

    // Nothing planned anywhere: not on the dashboard, not in the invoices list.
    // Read afresh, not from what the dashboard kept: without that, no request would go out.
    const fresh = page.waitForResponse((response) => response.url().endsWith('/api/dashboard'))
    await navigate(page, 'Übersicht')
    await fresh
    await expect(plannedEntry).toHaveCount(0)
    await navigate(page, 'Rechnungen')
    await page.getByPlaceholder('Rechnungsnummer oder Kunde suchen …').fill(invoice.invoice_number)
    await expect(plannedEntry).toHaveCount(0)
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
