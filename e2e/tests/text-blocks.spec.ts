import { expect, test } from '@playwright/test'
import { seedCustomer, seedPatient, seedTreatment } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Textbausteine (issues.md 11): a library of named snippets, and the way they reach the two
 * free texts of a Patientenbehandlung — at the cursor, not appended, because the vet builds a
 * Vorbericht out of them in the order the examination went.
 */
test.describe('text blocks', () => {
  test('a block is written once and inserted at the cursor', async ({ page, request }) => {
    // Unique per run: the specs share one database.
    const suffix = Date.now().toString().slice(-6)
    const blockName = `Impfung-${suffix}`
    const blockText = `Impfung nach Schema ${suffix}.`

    await signIn(page)

    // --- write the block ---
    await navigate(page, 'Textbausteine')
    await page.getByRole('button', { name: 'Neuer Textbaustein' }).click()
    await page.getByLabel('Name').fill(blockName)
    await page.getByLabel('Inhalt').fill(blockText)
    await page.getByLabel('Inhalt').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // It is findable by its text, not only by its name.
    await navigate(page, 'Textbausteine')
    await page.getByPlaceholder('Name oder Inhalt suchen …').fill(`nach Schema ${suffix}`)
    await expect(page.getByText(blockName).filter({ visible: true }).first()).toBeVisible()

    // --- insert it mid-sentence ---
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId, `Bello-${suffix}`)
    const { treatmentId } = await seedTreatment(request, patientId)
    // `patients[].id` is the Patientenbehandlung — the record that carries the two texts.
    const treatment = await request.get(`/api/treatments/${treatmentId}`)
    const record = (await treatment.json()).patients[0].id
    await page.goto(`/patient-treatments/${record}`)

    const reason = page.getByLabel('Vorbericht und Untersuchung')
    await reason.fill('AnfangEnde')
    // Put the caret between the two words; the picker steals focus, so the position has to
    // survive the dialog opening.
    await reason.evaluate((element: HTMLTextAreaElement) => element.setSelectionRange(6, 6))
    await reason.dispatchEvent('select')

    await page
      .getByRole('button', { name: 'Textbaustein einfügen' })
      .first()
      .click()
    await page.getByPlaceholder('Name oder Inhalt suchen …').fill(blockName)
    await page.getByRole('button', { name: new RegExp(blockName) }).click()

    // Landed at the caret, on a line of its own, and saved without a save button.
    await expect(reason).toHaveValue(`Anfang\n${blockText}Ende`)
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // And it survives a reload, which is the only proof auto-save actually wrote it.
    await page.reload()
    await expect(page.getByLabel('Vorbericht und Untersuchung')).toHaveValue(
      `Anfang\n${blockText}Ende`,
    )
  })
})
