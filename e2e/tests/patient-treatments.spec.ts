import { execFile } from 'node:child_process'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { expect, test } from '@playwright/test'
import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
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
  test('each animal has its own positions, reason and finding', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const eddie = await seedPatient(request, customerId, 'Eddie')
    const helene = await seedPatient(request, customerId, 'Helene')
    const drug = await seedDrug(request)
    const { treatmentId } = await seedTreatment(request, eddie)
    await request.post(`/api/treatments/${treatmentId}/patients`, {
      data: { patient_id: helene },
    })

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)

    // One group per animal, each with its own search box.
    const eddiesGroup = page.locator('div').filter({ has: page.getByRole('link', { name: 'Eddie' }) }).last()
    const helenesGroup = page.locator('div').filter({ has: page.getByRole('link', { name: 'Helene' }) }).last()

    await eddiesGroup.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(drug.drugName)
    await eddiesGroup.getByRole('option', { name: /· 10 ml/ }).first().click()
    await helenesGroup.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill('Beratung im')
    await helenesGroup.getByRole('option', { name: /Beratung/ }).first().click()

    // A line belongs to the animal whose box it was typed into.
    await expect(eddiesGroup.getByLabel('Name')).toHaveCount(1)
    await expect(eddiesGroup.getByLabel('Name')).toHaveValue(new RegExp(drug.drugName))
    await expect(helenesGroup.getByLabel('Name')).toHaveValue(/Beratung/)

    // Reason and finding are the animal's, on its own page.
    await page.getByRole('link', { name: 'Eddie' }).first().click()
    await expect(page).toHaveURL(/\/patient-treatments\/\d+$/)
    await page.getByLabel('Behandlungsgrund').fill('Verbandswechsel')
    await page.getByLabel('Befund').fill('Wunde sauber und trocken.')
    await page.getByLabel('Befund').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // That page shows this animal's positions, not the other's.
    await expect(page.getByLabel('Name')).toHaveCount(1)
    await expect(page.getByLabel('Name')).toHaveValue(new RegExp(drug.drugName))

    await page.getByRole('link', { name: /Zurück/ }).click()
    await expect(page.getByText('Verbandswechsel')).toBeVisible()
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
          treatment_reason: `Grund ${record.name}`,
          finding: `Befund ${record.name}`,
        },
      })
    }

    await signIn(page)
    await page.goto(`/treatments/${treatmentId}`)
    // The button is disabled until the lines are on screen.
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
