import { expect, test } from '@playwright/test'
import { login, seedCustomer, seedPatient, seedService, seedTreatment } from '../fixtures/seed'
import { pdfFonts, pdfText, signIn } from './helpers'

/**
 * Formatted text in an animal's record (migration `0030`): bold, italic, underline and lists in
 * "Vorbericht und Untersuchung" and "Therapie und weiteres Vorgehen", typography while typing,
 * and all of it on the invoice the customer receives.
 */
test.describe('formatted text', () => {
  test('the vet formats the record, and the invoice prints it', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const { serviceId } = await seedService(request)
    await request.post(`/api/treatments/${treatmentId}/items`, {
      data: { kind: 'service', service_id: serviceId, quantity: '1' },
    })
    const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()
    const visit = `/appointments/${treatment.appointment_id}?tier=${treatment.patients[0].id}`

    await signIn(page)
    await page.goto(visit)

    // The toolbar is what the phone has instead of shortcuts.
    const reason = page.getByLabel('Vorbericht und Untersuchung')
    const tools = page.getByRole('toolbar', { name: 'Formatierung' }).first()
    await reason.click()
    await tools.getByRole('button', { name: 'Fett' }).click()
    await expect(tools.getByRole('button', { name: 'Fett' })).toHaveAttribute('aria-pressed', 'true')
    await page.keyboard.type('Ohr')
    await tools.getByRole('button', { name: 'Fett' }).click()
    // Typography as it is typed: `->` is an arrow, quotes are German.
    await page.keyboard.type(' links -> "Otitis"')
    await page.keyboard.press('Enter')
    await tools.getByRole('button', { name: 'Nummerierte Liste' }).click()
    await page.keyboard.type('spülen')
    await page.keyboard.press('Enter')
    await page.keyboard.type('tropfen')

    // The finding by the keyboard, the way the vet types at the desk: Ctrl+B and Ctrl+I, and
    // `- ` starting a bullet list.
    const finding = page.getByLabel('Therapie und weiteres Vorgehen')
    await finding.click()
    await page.keyboard.press('Control+b')
    await page.keyboard.press('Control+i')
    await page.keyboard.type('Ruhe')
    await page.keyboard.press('Control+i')
    await page.keyboard.press('Control+b')
    await page.keyboard.press('Enter')
    await page.keyboard.type('- Kontrolle in 7 Tagen')
    await finding.blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // It is stored as written: a reload shows the same formatting.
    await page.reload()
    await expect(reason.locator('strong')).toHaveText('Ohr')
    await expect(reason.locator('p').first()).toHaveText('Ohr links → „Otitis“')
    await expect(reason.locator('ol > li')).toHaveText(['spülen', 'tropfen'])
    await expect(finding.locator('em')).toHaveText('Ruhe')
    await expect(finding.locator('strong')).toHaveText('Ruhe')
    await expect(finding.locator('ul > li')).toHaveText(['Kontrolle in 7 Tagen'])

    // The invoice prints the formatting, not the plain words.
    await login(request)
    const created = await request.post(`/api/treatments/${treatmentId}/invoice`, {
      data: { includes_finding: true },
    })
    expect(created.status()).toBe(200)
    const invoice = await created.json()
    const pdf = Buffer.from(await (await request.get(`/api/invoices/${invoice.id}/pdf`)).body())
    const text = await pdfText(pdf)
    expect(text).toContain('Vorbericht und Untersuchung: Ohr links → „Otitis“')
    expect(text).toMatch(/1\.\s+spülen/)
    expect(text).toMatch(/2\.\s+tropfen/)
    expect(text).toMatch(/•\s+Kontrolle in 7 Tagen/)
    // Bold italic is a face of its own in the embedded font, and nothing else on the invoice is
    // set in it: it is there because the finding asked for it. (Bold or italic alone would prove
    // less — the headings are bold and the service dates italic anyway.)
    expect(await pdfFonts(pdf)).toContain('SourceSans3-BoldIt')
  })

  test('a formatted text block keeps its list when it is inserted', async ({ page, request }) => {
    const suffix = Date.now().toString().slice(-6)
    const blockName = `Nachsorge-${suffix}`
    await login(request)
    const block = await (await request.post('/api/text-blocks')).json()
    await request.patch(`/api/text-blocks/${block.id}`, {
      data: {
        name: blockName,
        content: {
          type: 'doc',
          content: [
            {
              type: 'bulletList',
              content: ['Leine', 'Kragen'].map((text) => ({
                type: 'listItem',
                content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
              })),
            },
          ],
        },
      },
    })

    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { treatmentId } = await seedTreatment(request, patientId)
    const treatment = await (await request.get(`/api/treatments/${treatmentId}`)).json()

    await signIn(page)
    await page.goto(`/appointments/${treatment.appointment_id}?tier=${treatment.patients[0].id}`)
    await page.getByRole('button', { name: 'Textbaustein einfügen' }).last().click()
    await page.getByPlaceholder('Name oder Inhalt suchen …').fill(blockName)
    // The picker previews the words, not the markup.
    const entry = page.getByRole('button', { name: `${blockName} Leine Kragen` })
    await expect(entry).toBeVisible()
    await entry.click()

    const finding = page.getByLabel('Therapie und weiteres Vorgehen')
    await expect(finding.locator('ul > li')).toHaveText(['Leine', 'Kragen'])
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
  })
})
