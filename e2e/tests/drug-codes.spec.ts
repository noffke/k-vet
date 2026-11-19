import { expect, test } from '../fixtures/test'
import { ifaMatrix, uniquePzn } from '../fixtures/codes'
import { seedDrug } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * The codes on a drug's packs (migration 0038): scanned or typed into the drug page, shown as
 * people know them, and removed again — asked first. A scanner types and presses Enter, which
 * is what the test does.
 */
test.describe('codes on the drug', () => {
  test('a code is added by scanning, shown as a PZN, and removed', async ({ page, request }) => {
    const { drugId } = await seedDrug(request)
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)

    const pzn = uniquePzn()
    const scan = page.getByLabel('Code hinzufügen')
    // The PZN barcode (Code 39).
    await scan.fill(`-${pzn}`)
    await scan.press('Enter')
    const codes = page.getByRole('listitem')
    await expect(codes.filter({ hasText: `PZN ${pzn}` })).toHaveCount(1)

    // A Data Matrix of the same pack adds nothing new: only its product code counts, and that
    // is the same PZN. Its batch and expiry are not kept on the drug.
    await scan.fill(ifaMatrix(pzn, 'CH-77', '290800'))
    await scan.press('Enter')
    await expect(page.getByText(/Zuletzt gelesen: \[\)>␞06␝9N11/)).toBeVisible()
    await expect(codes.filter({ hasText: 'PZN' })).toHaveCount(1)
    await expect(codes.filter({ hasText: 'CH-77' })).toHaveCount(0)

    // Not a code: said so.
    await scan.fill('Metacam')
    await scan.press('Enter')
    await expect(page.getByText('Kein gültiger Code')).toBeVisible()

    await page.getByRole('button', { name: `Code PZN ${pzn} entfernen` }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(codes.filter({ hasText: 'PZN' })).toHaveCount(0)
  })
})
