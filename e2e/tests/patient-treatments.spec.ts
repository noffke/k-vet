import { execFile } from 'node:child_process'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { expect, test } from '@playwright/test'
import {
  doc,
  seedCustomer,
  seedDrug,
  seedPatient,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { signIn } from './helpers'

const run = promisify(execFile)

/** Reads a PDF's text with poppler's pdftotext. */
async function pdfText(bytes: Buffer): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'kvet-pdf-'))
  const file = join(dir, 'invoice.pdf')
  await writeFile(file, bytes)
  const { stdout } = await run('pdftotext', ['-layout', file, '-'])
  return stdout
}

/**
 * Two animals on one visit (issues.md): each has its own record, its own positions and its
 * own place on the invoice. A treatment with more than one animal had no coverage at all
 * before the positions belonged to the animal.
 */
test.describe('patient treatments', () => {
  test('each animal has its own tab: positions, reason and finding', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const eddie = await seedPatient(request, customerId, 'Eddie')
    const helene = await seedPatient(request, customerId, 'Helene')
    const drug = await seedDrug(request)
    const { serviceName } = await seedService(request, { perVisit: true })
    const { treatmentId } = await seedTreatment(request, eddie)
    await request.post(`/api/treatments/${treatmentId}/patients`, {
      data: { patient_id: helene },
    })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    const panel = page.getByRole('tabpanel')
    const picker = panel.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …')

    // Eddie's tab is open first: his line, his reason and finding.
    await expect(page.getByRole('tab', { name: 'Eddie' })).toHaveAttribute('aria-selected', 'true')
    // The tab bar scrolls sideways at most — it once grew a vertical scrollbar of its own.
    const tabBar = page.getByRole('tablist')
    expect(await tabBar.evaluate((list) => list.scrollHeight - list.clientHeight)).toBe(0)
    await picker.fill(drug.drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()
    await expect(panel.getByLabel('Name')).toHaveValue(new RegExp(drug.drugName))
    await panel.getByLabel('Vorbericht und Untersuchung').fill('Verbandswechsel')
    await panel.getByLabel('Therapie und weiteres Vorgehen').fill('Wunde sauber und trocken.')
    await panel.getByLabel('Therapie und weiteres Vorgehen').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // The visit's own position, picked in Eddie's tab like any other: its service is marked
    // "pro Termin", so it goes to the visit by itself — there is no "Ohne Tier" to choose.
    await expect(panel.getByRole('button', { name: 'Ohne Tier' })).toHaveCount(0)
    await picker.fill(serviceName)
    await page.getByRole('option', { name: new RegExp(serviceName) }).first().click()
    await expect(page.getByText(`„${serviceName}“ pro Termin hinzugefügt`)).toBeVisible()
    await expect(panel.getByText('pro Termin')).toBeVisible()
    await expect(panel.getByLabel('Name')).toHaveCount(2)
    // Only Eddie's line can move — between the two animals, never to the visit.
    // By role: the label wraps the select, so its text content carries the options as well.
    const owner = panel.getByRole('combobox', { name: 'Patient', exact: true })
    await expect(owner).toHaveCount(1)
    expect(await owner.locator('option').allTextContents()).toEqual(['Eddie', 'Helene'])

    // Helene's tab: her own record and lines — and the same "pro Termin" line, so the vet sees
    // the house call is already entered.
    await page.getByRole('tab', { name: 'Helene' }).click()
    await expect(panel.getByLabel('Vorbericht und Untersuchung')).toHaveText('')
    await picker.fill('Beratung im')
    await page.getByRole('option', { name: /Beratung/ }).first().click()
    await expect(panel.getByLabel('Name')).toHaveCount(2)
    await expect(panel.getByLabel('Name').first()).toHaveValue(/Beratung/)
    await expect(panel.getByLabel('Name').last()).toHaveValue(new RegExp(serviceName))

    // Back to Eddie: what was typed is still there.
    await page.getByRole('tab', { name: 'Eddie' }).click()
    await expect(panel.getByLabel('Vorbericht und Untersuchung')).toHaveText('Verbandswechsel')

    // Three lines in all — the visit's one counted once, not per animal.
    const lines = (await (await request.get(`/api/treatments/${treatmentId}/items`)).json()) as {
      patient_treatment_id: number | null
    }[]
    expect(lines).toHaveLength(3)
    expect(lines.filter((line) => line.patient_treatment_id === null)).toHaveLength(1)
  })

  test("a warning sits in its animal's tab, and the tab says so while closed", async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const calm = await seedPatient(request, customerId, 'Anton')
    const biter = await seedPatient(request, customerId, 'Bruno')
    await request.patch(`/api/patients/${biter}`, {
      data: { warning_remark: 'Beißt bei der Blutabnahme' },
    })
    const { treatmentId } = await seedTreatment(request, calm)
    await request.post(`/api/treatments/${treatmentId}/patients`, { data: { patient_id: biter } })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)

    // Anton's tab is open: no banner — but Bruno's tab carries the mark.
    await expect(page.getByRole('tab', { name: /Anton/ })).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect(page.getByRole('tab', { name: /Bruno/ }).getByRole('img', { name: 'Warnhinweis' })).toBeVisible()

    await page.getByRole('tab', { name: /Bruno/ }).click()
    await expect(page.getByRole('tabpanel').getByRole('alert')).toContainText('Beißt bei der Blutabnahme')
  })

  test("the animal's visit history opens the visit on its tab", async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const first = await seedPatient(request, customerId, 'Erna')
    const second = await seedPatient(request, customerId, 'Fritz')
    const { treatmentId } = await seedTreatment(request, first)
    await request.post(`/api/treatments/${treatmentId}/patients`, { data: { patient_id: second } })

    await signIn(page)
    await page.goto(`/patients/${second}`)
    await page.getByRole('link', { name: /\d{2}\.\d{2}\.\d{4}/ }).first().click()

    // Fritz's tab, not the first one.
    await expect(page).toHaveURL(/\/appointments\/\d+\?tier=\d+/)
    await expect(page.getByRole('tab', { name: /Fritz/ })).toHaveAttribute('aria-selected', 'true')
  })

  test('the invoice groups the positions and the report per animal', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const eddie = await seedPatient(request, customerId, 'Eddie')
    const helene = await seedPatient(request, customerId, 'Helene')
    const { treatmentId } = await seedTreatment(request, eddie)
    const attached = await (
      await request.post(`/api/treatments/${treatmentId}/patients`, {
        data: { patient_id: helene },
      })
    ).json()

    // A line and a reason for each animal.
    const service = await (await request.get('/api/services?q=Beratung im')).json()
    for (const record of attached.patients) {
      await request.post(`/api/treatments/${treatmentId}/items`, {
        data: {
          kind: 'service',
          service_id: service[0].id,
          quantity: '1',
          patient_treatment_id: record.id,
        },
      })
      await request.patch(`/api/patient-treatments/${record.id}`, {
        data: {
          treatment_reason: doc(`Grund ${record.name}`),
          finding: doc(`Befund ${record.name}`),
        },
      })
    }

    // And the visit's own position: last on the invoice, once.
    const { serviceId, serviceName } = await seedService(request, { perVisit: true })
    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: { kind: 'service', service_id: serviceId, quantity: '1' },
    })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    // The button is disabled until the lines are on screen: the open tab's and the visit's.
    await expect(page.getByLabel('Name')).toHaveCount(2)
    await page.getByRole('button', { name: 'Rechnung erstellen' }).click()
    const openedTab = page.context().waitForEvent('page')
    await page.getByRole('button', { name: 'Rechnung erstellen' }).last().click()
    await (await openedTab).close()
    await expect(page.getByText(/\d{4}-\d{4}/).first()).toBeVisible()

    const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()
    const pdf = await request.get(`/api/invoices/${treatment.invoice.id}/pdf`)
    const text = await pdfText(await pdf.body())

    // Positions under a heading per animal, as on the practice's own invoices.
    expect(text).toContain('Tier: Eddie')
    expect(text).toContain('Tier: Helene')
    // The visit's position comes after every animal, and only once.
    expect(text.split(serviceName)).toHaveLength(2)
    expect(text.indexOf(serviceName)).toBeGreaterThan(text.indexOf('Tier: Helene'))
    // And the report iterates them, each with its own reason and finding.
    for (const name of ['Eddie', 'Helene']) {
      expect(text).toContain(`Grund ${name}`)
      expect(text).toContain(`Befund ${name}`)
    }
  })

  test('a drug line cannot be left without an animal', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId, 'Rufus')
    const drug = await seedDrug(request)
    const { treatmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(drug.drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()
    await expect(page.getByLabel('Name')).toHaveValue(new RegExp(drug.drugName))

    // Which animal received which batch is what the ownership is for, so the API refuses.
    const line = await (await request.get(`/api/treatments/${treatmentId}/items`)).json()
    const response = await request.patch(`/api/treatment-items/${line[0].id}`, {
      data: { patient_treatment_id: null },
    })
    expect(response.status()).toBe(422)
  })
})
