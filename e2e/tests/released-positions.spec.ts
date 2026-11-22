import {
  login,
  seedCustomer,
  seedDrug,
  seedPatient,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { type APIRequestContext, expect, type Page, test } from '../fixtures/test'
import { editValue, signIn } from './helpers'

/**
 * The positions of an invoice that went out stay editable (2026-11-22): the appointment then says
 * "PDF veraltet", and "Rechnung aktualisieren" issues the corrected invoice — a new number, the
 * old one cancelled — as often as it takes. The shelf follows the lines all along.
 */
async function release(request: APIRequestContext, treatmentId: number): Promise<string> {
  const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()
  const invoiceId = treatment.invoice.id as number
  const accepted = await request.post(`/api/invoices/${invoiceId}/accept`, {
    data: { recipient_emails: [], send_at: new Date().toISOString(), template: 'Standard' },
  })
  expect(accepted.ok()).toBe(true)
  // On paper: the invoice is with the customer.
  const posted = await request.post(`/api/invoices/${invoiceId}/mark-posted`)
  expect(posted.ok()).toBe(true)
  return treatment.invoice.invoice_number as string
}

async function stock(request: APIRequestContext, drugId: number): Promise<string> {
  return (await (await request.get(`/api/drugs/${drugId}`)).json()).in_stock as string
}

/** "Rechnung aktualisieren" from the banner, through the invoice page, back to the visit. */
async function update(page: Page): Promise<void> {
  const stale = page.getByRole('alert').filter({ hasText: 'PDF veraltet' })
  await stale.getByRole('button', { name: 'Rechnung aktualisieren' }).click()
  const remade = page.context().waitForEvent('page')
  await page.getByRole('button', { name: 'Rechnung aktualisieren' }).last().click()
  await (await remade).close()
  await expect(page).toHaveURL(/\/appointments\/\d+/)
  await expect(stale).toHaveCount(0)
}

test('a sent invoice is corrected through its positions, round after round', async ({
  page,
  request,
}) => {
  const { customerId } = await seedCustomer(request)
  const { treatmentId } = await seedTreatment(
    request,
    await seedPatient(request, customerId, `Korrektur-${Date.now() % 1e6}`),
  )
  const { drugId, drugName, packagingId } = await seedDrug(request)
  const { serviceId } = await seedService(request)
  await login(request)
  for (const line of [
    { kind: 'drug_packaging', drug_packaging_id: packagingId, quantity: '10' },
    { kind: 'service', service_id: serviceId, quantity: '1' },
  ]) {
    expect((await request.post(`/api/treatments/${treatmentId}/items`, { data: line })).ok()).toBe(
      true,
    )
  }
  expect((await request.post(`/api/treatments/${treatmentId}/invoice`, { data: {} })).ok()).toBe(
    true,
  )
  const numbers = [await release(request, treatmentId)]
  expect(await stock(request, drugId)).toBe('990.00')

  const stale = page.getByRole('alert').filter({ hasText: 'PDF veraltet' })
  for (const [round, quantity, left] of [
    [1, '20', '980.00'],
    [2, '30', '970.00'],
  ] as const) {
    await signIn(page, `/treatments/${treatmentId}`)
    await expect(stale).toHaveCount(0)

    // Sent — and the quantity can still be changed; the appointment says the PDF is outdated.
    const field = await editValue(page, drugName, 'Menge')
    await field.fill(quantity)
    await field.press('Escape')
    await expect(stale).toBeVisible()
    await expect.poll(() => stock(request, drugId)).toBe(left)

    await update(page)
    const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()
    expect(treatment.invoice.status, `round ${round}`).toBe('created')
    expect(numbers, `round ${round}: a new number`).not.toContain(treatment.invoice.invoice_number)
    // The update replaces the invoice, it does not undo the treatment: the shelf stays.
    expect(await stock(request, drugId)).toBe(left)

    numbers.push(await release(request, treatmentId))
  }
  expect(new Set(numbers).size).toBe(3)
})
