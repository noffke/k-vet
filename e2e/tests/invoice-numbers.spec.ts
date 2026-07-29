import { expect, type Page, test } from '@playwright/test'
import { seedCustomer, seedPatient, seedService, seedTreatment } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * Invoice number discipline (T039, quickstart #6, SC-006): a number is issued at most once,
 * updating a created invoice keeps it, and updating an accepted one burns it.
 */

/** The invoice number currently shown on the treatment page. */
async function shownNumber(page: Page): Promise<string> {
  const text = await page.getByText(/\d{4}-\d{4}/).first().innerText()
  return text.match(/\d{4}-\d{4}/)?.[0] ?? ''
}

/** A treatment with one billable line, ready to be invoiced through the UI. */
async function billableTreatment(page: Page, request: Parameters<typeof seedCustomer>[0]) {
  const { customerId } = await seedCustomer(request)
  const patientId = await seedPatient(request, customerId)
  const { treatmentId } = await seedTreatment(request, patientId)
  const { serviceId } = await seedService(request)
  await request.post(`/api/treatments/${treatmentId}/items`, {
    data: { kind: 'service', service_id: serviceId, quantity: '1' },
  })
  await page.goto(`/treatments/${treatmentId}`)
  return treatmentId
}

test('a created invoice keeps its number, an accepted one burns it', async ({ page, request }) => {
  await signIn(page)
  await billableTreatment(page, request)

  // ── First issue ─────────────────────────────────────────────────────────────
  await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
  await page.getByRole('button', { name: 'Rechnung erstellen' }).last().click()
  await expect(page.getByText('Erstellt').first()).toBeVisible()
  const first = await shownNumber(page)

  // ── Updating while still created: same number, no waste ─────────────────────
  await page.getByRole('button', { name: 'Rechnung aktualisieren' }).click()
  await page.getByLabel('Befund aufführen').check()
  await page.getByRole('button', { name: 'Rechnung aktualisieren' }).last().click()
  await expect(page.getByText('Erstellt').first()).toBeVisible()
  expect(await shownNumber(page)).toBe(first)

  // ── Accept, then update: the accepted invoice is cancelled and a new number
  //    is issued — the old one is never used again (FR-032) ───────────────────
  await page.getByRole('button', { name: 'Rechnung freigeben' }).click()
  await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()
  await expect(page.getByText('Freigegeben').first()).toBeVisible()

  await expect(page.getByText(/Beim Aktualisieren wird sie storniert/).first()).toBeVisible()
  await page.getByRole('button', { name: 'Rechnung aktualisieren' }).click()
  await page.getByRole('button', { name: 'Rechnung aktualisieren' }).last().click()

  await expect(page.getByText('Erstellt').first()).toBeVisible()
  const second = await shownNumber(page)
  expect(second).not.toBe(first)

  // The burned number belongs to exactly one cancelled invoice.
  const burned = await request.get(`/api/invoices?q=${encodeURIComponent(first)}&cancelled=true`)
  const cancelled = await burned.json()
  expect(cancelled).toHaveLength(1)
  expect(cancelled[0].status).toBe('cancelled')
  expect(cancelled[0].ts_cancelled).not.toBeNull()

  // ── The next invoice continues the counter; nothing is reissued ─────────────
  await billableTreatment(page, request)
  await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
  await page.getByRole('button', { name: 'Rechnung erstellen' }).last().click()
  const third = await shownNumber(page)
  expect(new Set([first, second, third]).size).toBe(3)

  const counter = (value: string) => Number(value.split('-')[1])
  expect(counter(second)).toBeGreaterThan(counter(first))
  expect(counter(third)).toBeGreaterThan(counter(second))

  // The pattern is the configured one: four-digit counter, scoped to the year.
  const year = String(new Date().getFullYear())
  for (const number of [first, second, third]) {
    expect(number).toMatch(new RegExp(`^${year}-\\d{4}$`))
  }
})
