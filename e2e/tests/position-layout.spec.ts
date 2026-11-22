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
 * on an appointment and on a Preisauskunft. A long name wraps; a long unit widens its column,
 * open or closed.
 */
const LONG = 'Amoxicillin/Clavulansäure 400/100 mg Filmtabletten für Hunde und Katzen'
/** The longest unit in the practice's catalogue: a drug sold as whole packs (Cortavance). */
const UNIT = 'Originalpackung'
const SEARCH = 'Medikament, Leistung oder Gruppe suchen …'

/**
 * `inner` lies within `outer` on both axes. Measured until the page holds still: the two boxes
 * are read one after the other, and the picker's results — open while the search keeps the focus
 * after a service — may scroll the page in between.
 */
async function expectWithin(inner: Locator, outer: Locator): Promise<void> {
  await expect(async () => {
    await expectInside(inner, outer)
    const [box, frame] = await Promise.all([inner.boundingBox(), outer.boundingBox()])
    expect(
      box && frame && box.y >= frame.y - 0.5 && box.y + box.height <= frame.y + frame.height + 0.5,
    ).toBe(true)
  }).toPass({ timeout: 5_000 })
}

async function checkRow(page: Page, name: RegExp): Promise<void> {
  const row = position(page, name)
  await expectNoSidewaysScroll(page)
  // The amount and the bin stay in the row, beside the name however it wraps.
  await expectWithin(row.getByText(/€/).first(), row)
  await expectWithin(row.getByRole('button', { name: /Löschen|Position entfernen/ }), row)
  const list = row.locator('xpath=ancestor::ul[1]')
  await expect(async () => {
    const [rowBox, listBox] = await Promise.all([row.boundingBox(), list.boundingBox()])
    expect(rowBox && listBox && rowBox.width <= listBox.width + 0.5).toBe(true)
  }).toPass({ timeout: 5_000 })

  // Open, the unit sits inside its field, and the field stays clear of the amount beside it
  // (a fixed width once let "Originalpackung" run out of it over the price); closed again, the
  // unit sits inside its cell.
  const quantity = await editValue(page, name, 'Menge')
  await expectInside(row.getByText(UNIT, { exact: true }), quantity)
  if (await page.evaluate(() => window.matchMedia('(min-width: 640px)').matches)) {
    // On a laptop the amount is the next column; on a phone it sits on the line above.
    await expect(async () => {
      const [field, amount] = await Promise.all([
        quantity.boundingBox(),
        row.getByText(/€/).first().boundingBox(),
      ])
      expect(
        field && amount && field.x + field.width <= amount.x + 0.5,
        'clear of the amount',
      ).toBe(true)
    }).toPass({ timeout: 5_000 })
  }
  await expectNoSidewaysScroll(page)
  await quantity.press('Escape')
  const cell = row.getByRole('button', { name: /^Menge ändern:/ })
  await expect(cell).toContainText(UNIT)
  await expectInside(cell.getByText(new RegExp(UNIT)), row)
}

test.describe('positions with a long name and a long unit', () => {
  test('fit their rows on an appointment and on a Preisauskunft', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(
      request,
      await seedPatient(request, customerId, `Lang-${Date.now() % 1e6}`),
    )
    const { drugId } = await seedDrug(request, { unit: UNIT })
    const name = `${LONG} ${Date.now() % 1e6}`
    await request.patch(`/api/drugs/${drugId}`, { data: { name } })
    const pattern = new RegExp(name.slice(-20))

    await signIn(page, `/treatments/${treatmentId}`)
    await page.getByPlaceholder(SEARCH).fill(name.slice(0, 30))
    await page.getByRole('option', { name: pattern }).first().click()
    await expect(position(page, pattern)).toHaveCount(1)
    // Another line picked after it, so the long one is no longer the newest and reads as text.
    await page.getByPlaceholder(SEARCH).fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    await expect(position(page, /Beratung/)).toHaveCount(1)
    await checkRow(page, pattern)

    await page.goto('/quote')
    await page.getByPlaceholder(SEARCH).fill(name.slice(0, 30))
    await page.getByRole('option', { name: pattern }).first().click()
    await page.getByPlaceholder(SEARCH).fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    await expect(position(page, /Beratung/)).toHaveCount(1)
    await checkRow(page, pattern)
  })
})

test.describe('a position being typed into', () => {
  test('a service picked leaves the focus in the search; opened, its subline clears the focus ring', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(
      request,
      await seedPatient(request, customerId, `Satz-${Date.now() % 1e6}`),
    )
    await signIn(page, `/treatments/${treatmentId}`)
    const search = page.getByPlaceholder(SEARCH)
    await search.fill('Beratung im')
    await page
      .getByRole('option', { name: /Beratung/ })
      .first()
      .click()
    const row = position(page, /Beratung/)

    // A service mostly stays 1 × at 100 % (2026-11-22): it comes in closed, and the vet types
    // the next position straight away.
    await expect(row).toHaveCount(1)
    await expect(search).toBeFocused()
    await expect(row.getByLabel('Menge', { exact: true })).toHaveCount(0)

    // Tapped, its quantity opens with GOT and VAT in the subline under it.
    const quantity = await editValue(page, /Beratung/, 'Menge')
    await expect(quantity).toBeFocused()
    const subline = row.getByText(/GOT \d+ · \d+\s%\sUSt\./)
    await expect(subline).toBeVisible()
    // Clear of the focus ring: 2 px outline at 2 px offset around the field's box.
    // (The input fills its box either way: its own border, or the box of a field with a unit.)
    const [box, text] = await Promise.all([quantity.boundingBox(), subline.boundingBox()])
    expect(box && text && box.y + box.height + 4 <= text.y + 0.5).toBe(true)
  })

  test('a drug picked opens its quantity, and its details do not repeat the VAT', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(
      request,
      await seedPatient(request, customerId, `Satz-${Date.now() % 1e6}`),
    )
    const { drugName } = await seedDrug(request)
    await signIn(page, `/treatments/${treatmentId}`)
    await page.getByPlaceholder(SEARCH).fill(drugName)
    await page
      .getByRole('option', { name: new RegExp(drugName) })
      .first()
      .click()
    const row = position(page, drugName)

    // A drug's quantity is nearly always typed: the line just picked has it open and focused.
    await expect(row.getByLabel('Menge', { exact: true })).toBeFocused()
    await expect(row.getByText(/%\sUSt\./)).toHaveCount(1)
    // Opened while typed into, the details do not say it a second time.
    await openPosition(page, drugName)
    await expect(row.getByText(/%\sUSt\./)).toHaveCount(1)
  })
})
