import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
import { expect, type Locator, type Page, test } from '../fixtures/test'
import {
  editValue,
  expectInside,
  expectNoSidewaysScroll,
  openPosition,
  position,
  signIn,
} from './helpers'

/**
 * The positions as compact rows (`PositionRow`): however long a drug's name or its unit, nothing
 * spills out of its row and the page never scrolls sideways — on the laptop and on the phone,
 * on an appointment and on a Preisauskunft. A long name wraps; a long unit widens its column.
 */
const LONG = 'Amoxicillin/Clavulansäure 400/100 mg Filmtabletten für Hunde und Katzen'

/** `inner` lies within `outer` on both axes. */
async function expectWithin(inner: Locator, outer: Locator): Promise<void> {
  await expectInside(inner, outer)
  const [box, frame] = await Promise.all([inner.boundingBox(), outer.boundingBox()])
  expect(
    box && frame && box.y >= frame.y - 0.5 && box.y + box.height <= frame.y + frame.height + 0.5,
  ).toBe(true)
}

async function checkRow(page: Page, name: RegExp): Promise<void> {
  const row = position(page, name)
  await expectNoSidewaysScroll(page)
  // The amount and the bin stay in the row, beside the name however it wraps.
  await expectWithin(row.getByText(/€/).first(), row)
  await expectWithin(row.getByRole('button', { name: /Löschen|Position entfernen/ }), row)
  const list = row.locator('xpath=ancestor::ul[1]')
  const [rowBox, listBox] = await Promise.all([row.boundingBox(), list.boundingBox()])
  expect(rowBox && listBox && rowBox.width <= listBox.width + 0.5).toBe(true)

  // Open, the unit sits inside its field; closed again, inside its cell.
  const quantity = await editValue(page, name, 'Menge')
  await expectInside(row.getByText('Tablette', { exact: true }), quantity)
  await expectNoSidewaysScroll(page)
  await quantity.press('Escape')
  const cell = row.getByRole('button', { name: /^Menge ändern:/ })
  await expect(cell).toContainText('Tablette')
  await expectInside(cell.getByText(/Tablette/), row)
}

test.describe('positions with a long name and a long unit', () => {
  test('fit their rows on an appointment and on a Preisauskunft', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(
      request,
      await seedPatient(request, customerId, `Lang-${Date.now() % 1e6}`),
    )
    const { drugId } = await seedDrug(request, { unit: 'Tablette' })
    const name = `${LONG} ${Date.now() % 1e6}`
    await request.patch(`/api/drugs/${drugId}`, { data: { name } })
    const pattern = new RegExp(name.slice(-20))

    await signIn(page, `/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(name.slice(0, 30))
    await page.getByRole('option', { name: pattern }).first().click()
    await expect(position(page, pattern)).toHaveCount(1)
    // Another line picked after it, so the long one is no longer the newest and reads as text.
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    await expect(position(page, /Beratung/)).toHaveCount(1)
    await checkRow(page, pattern)

    await page.goto('/quote')
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill(name.slice(0, 30))
    await page.getByRole('option', { name: pattern }).first().click()
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    await expect(position(page, /Beratung/)).toHaveCount(1)
    await checkRow(page, pattern)
  })
})

test.describe('a position being typed into', () => {
  test('says its GOT number and VAT once, and its subline clears the focus ring', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(
      request,
      await seedPatient(request, customerId, `Satz-${Date.now() % 1e6}`),
    )
    await signIn(page, `/treatments/${treatmentId}`)
    await page.getByPlaceholder('Medikament, Leistung oder Gruppe suchen …').fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    const row = position(page, /Beratung/)

    // The line just picked: its values open, GOT and VAT in the subline under them.
    const quantity = row.getByLabel('Menge', { exact: true })
    await expect(quantity).toBeFocused()
    const subline = row.getByText(/GOT \d+ · \d+\s%\sUSt\./)
    await expect(subline).toBeVisible()
    // Clear of the focus ring: 2 px outline at 2 px offset around the field's box.
    // (The input fills its box either way: its own border, or the box of a field with a unit.)
    const [box, text] = await Promise.all([quantity.boundingBox(), subline.boundingBox()])
    expect(box && text && box.y + box.height + 4 <= text.y + 0.5).toBe(true)

    // Opened while typed into, the details do not say it a second time.
    await openPosition(page, /Beratung/)
    await expect(row.getByText(/%\sUSt\./)).toHaveCount(1)
  })
})
