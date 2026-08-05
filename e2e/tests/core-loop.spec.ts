import { expect, test } from '@playwright/test'
import { seedCustomer, seedDrug, seedPatient, seedService } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * The core loop (T039, quickstart #1, SC-001): appointment → treatment → a stocked drug line
 * and a GOT service line from the picker → invoice with the finding → PDF → accept and email.
 *
 * Master data is seeded through the API (its own screens are covered by master-data.spec.ts);
 * everything from the appointment onwards is driven through the UI, because that is the path
 * the vet walks several times a day.
 */
test('a visit is recorded and its invoice accepted', async ({ page, request }) => {
  const { customerId, lastName } = await seedCustomer(request)
  // Unique per run: the specs share one database, and the picker searches all patients.
  const patientName = `Rex-${Date.now().toString().slice(-6)}`
  await seedPatient(request, customerId, patientName)
  const { drugName, packagingId } = await seedDrug(request)
  const { serviceName } = await seedService(request)
  await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
    data: { packages_received: 1, batch_number: 'CH-KERN', expiration_date: '2027-12-31' },
  })

  await signIn(page)

  // ── Appointment for today ────────────────────────────────────────────────────
  await navigate(page, 'Termine')
  await page.getByRole('button', { name: 'Neuer Termin' }).click()
  const today = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  const todayText = `${pad(today.getDate())}.${pad(today.getMonth() + 1)}.${today.getFullYear()}`
  // The date field opens on today, so the vet only types the time.
  await expect(page.getByLabel('Datum')).toHaveValue(todayText)
  await page.getByLabel('Uhrzeit').fill('09:30')
  await expect(page.getByRole('status')).toHaveText('Gespeichert')

  // ── A treatment, with the patient attached on its page ──────────────────────
  await page.getByRole('button', { name: 'Behandlung hinzufügen' }).click()
  await expect(page).toHaveURL(/\/treatments\/\d+$/)
  await page.getByPlaceholder('Patienten').fill(patientName)
  await page.getByRole('option', { name: new RegExp(patientName) }).first().click()

  await page.getByLabel('Behandlungsgrund').fill('Jahresimpfung und Kontrolle')
  await page.getByLabel('Befund').fill('Allgemeinzustand unauffällig, Gewicht stabil.')
  await expect(page.getByRole('status')).toHaveText('Gespeichert')

  // ── Two lines from the picker: a stocked drug and a service ──────────────────
  const picker = page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …')
  await picker.fill(drugName)
  await page.getByRole('option', { name: /· 10 ml/ }).first().click()
  await picker.fill(serviceName)
  await page.getByRole('option', { name: new RegExp(serviceName) }).first().click()

  const lines = page.getByLabel('Name')
  await expect(lines).toHaveCount(2)
  // Net 2,00 € for the 10 ml subset (AMPreisV § 4) plus 23,62 € for the service = 25,62 €,
  // plus 19 % VAT = 4,87 €.
  await expect(page.getByText('30,49 €').first()).toBeVisible()

  // ── Invoice with the finding printed ────────────────────────────────────────
  await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
  await page.getByLabel('Befund aufführen').check()
  await page.getByRole('button', { name: 'Rechnung erstellen' }).last().click()

  // The panel shows the number and the status in one line, so read the number out of it.
  const panelText = await page.getByText(/\d{4}-\d{4}/).first().innerText()
  const invoiceNumber = panelText.match(/\d{4}-\d{4}/)?.[0] ?? ''
  expect(invoiceNumber).toMatch(/^\d{4}-\d{4}$/)
  await expect(page.getByText('Erstellt').first()).toBeVisible()

  // The PDF is there before anything is accepted, and it carries the finding.
  const pdfLink = page.getByRole('link', { name: 'PDF' }).filter({ visible: true }).first()
  const href = await pdfLink.getAttribute('href')
  const pdf = await page.request.get(href ?? '')
  expect(pdf.status()).toBe(200)
  expect(pdf.headers()['content-type']).toBe('application/pdf')

  // ── Accept it, with the customer's address as recipient ─────────────────────
  await page.getByRole('button', { name: 'Rechnung freigeben' }).click()
  await expect(page.getByLabel('erika@example.com')).toBeChecked()
  await page.getByRole('button', { name: 'Rechnung freigeben' }).last().click()

  await expect(page.getByText('Freigegeben').first()).toBeVisible()
  await expect(page.getByText(invoiceNumber).first()).toBeVisible()

  // ── What the acceptance means underneath ────────────────────────────────────
  const listed = await request.get(`/api/invoices?q=${encodeURIComponent(invoiceNumber)}`)
  const [invoice] = await listed.json()
  expect(invoice.status).toBe('accepted')
  expect(invoice.ts_accepted).not.toBeNull()
  expect(invoice.customer_name).toContain(lastName)
  expect(invoice.patients).toEqual([patientName])

  // The dispense is frozen, so the stock movement can no longer be rewritten.
  const detail = await request.get(`/api/invoices/${invoice.id}`)
  expect((await detail.json()).email_recipients).toEqual(['erika@example.com'])
  const lots = await request.get(`/api/lots?packaging_id=${packagingId}`)
  const [lot] = await lots.json()
  expect(lot.batch_number).toBe('CH-KERN')
  expect(Number(lot.remaining)).toBe(90)
})
