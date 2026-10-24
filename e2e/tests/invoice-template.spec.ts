import { expect, test } from '@playwright/test'
import {
  markEmailed,
  seedAcceptedInvoice,
  seedCustomer,
  seedPatient,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * The invoice email templates listed in `config.toml` — here "Standard", "Kurz" and "Fehlt",
 * whose file is not in the templates folder (`global-setup.ts`). The first is the default; a
 * template without its file is offered disabled, with the reason.
 */
test.describe('choosing the email template', () => {
  test('the vet picks the template, and the appointment says which', async ({ page, request }) => {
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
    await expect(page.getByLabel('Standard', { exact: true })).toBeChecked()
    // Listed in config.toml, but its file is not in the folder: not choosable, and says why.
    await expect(page.getByLabel(/^Fehlt/)).toBeDisabled()
    await expect(
      page.getByText('Datei „invoice-email-fehlt.txt“ fehlt im Vorlagenordner.'),
    ).toBeVisible()

    // "Später", so the mail is still waiting when the appointment is looked at.
    await page.getByLabel('Kurz', { exact: true }).check()
    await page.getByLabel('Später versenden').check()
    await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
    await expect(page.getByText('Vorlage: Kurz')).toBeVisible()

    // "E-Mail senden" reopens on the template the mail waits with.
    await page.goto(`/treatments/${treatmentId}/invoice/send`)
    await expect(page.getByLabel('Kurz', { exact: true })).toBeChecked()
  })

  test('a delivered invoice says which template it went out with', async ({ page, request }) => {
    const invoice = await seedAcceptedInvoice(request)
    await markEmailed(invoice.invoiceId, 'erika@example.com', 'Kurz')

    await signIn(page)
    await page.goto(`/treatments/${invoice.treatmentId}`)
    await expect(page.getByText('An erika@example.com · Vorlage: Kurz')).toBeVisible()
  })
})
