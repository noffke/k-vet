import { ifaMatrix, uniquePzn } from '../fixtures/codes'
import { login, seedDrug } from '../fixtures/seed'
import { type APIRequestContext, expect, type Page, test } from '../fixtures/test'
import { navigate, signIn } from './helpers'

/**
 * "Lieferung einbuchen": a wholesaler's box booked pack by pack with a barcode scanner. The
 * scanner is a keyboard that types the code and presses Enter, which is what the test does.
 */

async function scan(page: Page, code: string) {
  const field = page.getByLabel('Packung scannen')
  await field.fill(code)
  await field.press('Enter')
}

/** A drug with no stock yet, whose packs carry a fresh PZN. */
async function drugWithCode(request: APIRequestContext) {
  const drug = await seedDrug(request, { packages: 0 })
  const pzn = uniquePzn()
  await login(request)
  const added = await request.post(`/api/drugs/${drug.drugId}/codes`, { data: { code: pzn } })
  expect(added.ok()).toBeTruthy()
  return { ...drug, pzn }
}

async function openDelivery(page: Page) {
  await signIn(page)
  await navigate(page, 'Apotheke')
  await page.getByRole('button', { name: 'Lieferung einbuchen' }).click()
  await expect(page).toHaveURL(/\/pharmacy\/delivery$/)
}

test.describe('booking a delivery by scanner', () => {
  test('a pack, its Data Matrix and a second pack are booked with Enter', async ({
    page,
    request,
  }) => {
    const { drugName, pzn } = await drugWithCode(request)
    await openDelivery(page)

    await scan(page, `-${pzn}`)
    await expect(page.getByRole('heading', { name: drugName })).toBeVisible()
    // The same pack's Data Matrix: batch and expiry, still one pack.
    await scan(page, ifaMatrix(pzn, 'CH-D1', '290800'))
    await expect(page.getByLabel('Chargennummer')).toHaveValue('CH-D1')
    await expect(page.getByLabel('Verfallsdatum')).toHaveValue('31.08.2029')
    await expect(page.getByLabel('Anzahl Packungen')).toHaveValue('1')
    // A second pack of the same kind counts up.
    await scan(page, `-${pzn}`)
    await expect(page.getByLabel('Anzahl Packungen')).toHaveValue('2')

    await expect(page.getByRole('button', { name: 'Einbuchen' })).toBeDisabled()
    await page.getByRole('button', { name: 'Stimmt' }).click()
    // Enter on the empty scan field books the pack in hand.
    await page.getByLabel('Packung scannen').press('Enter')

    const booked = page.getByRole('listitem').filter({ hasText: drugName })
    await expect(booked).toContainText('2 Pck.')
    await expect(booked).toContainText('CH-D1')
    await expect(page.getByRole('heading', { name: drugName })).toHaveCount(0)

    // The next pack of the same drug: its list price was checked in this delivery already.
    await scan(page, `-${pzn}`)
    await expect(page.getByText(/in dieser Lieferung schon bestätigt/)).toBeVisible()
    await expect(page.getByRole('button', { name: 'Einbuchen' })).toBeEnabled()
    await page.getByRole('button', { name: 'Verwerfen' }).click()

    // Another drug's pack while one is in hand waits.
    await scan(page, `-${pzn}`)
    const other = await drugWithCode(request)
    await scan(page, `-${other.pzn}`)
    await expect(page.getByRole('alert')).toContainText(`Erst „${drugName}“ einbuchen`)
  })

  test('an unknown code is added to a drug, booked, and taken back', async ({ page, request }) => {
    const { drugId, drugName } = await seedDrug(request, { packages: 0 })
    const pzn = uniquePzn()
    await openDelivery(page)

    await scan(page, ifaMatrix(pzn, 'CH-NEU', '300100'))
    await expect(page.getByText(`Unbekannter Code PZN ${pzn}`)).toBeVisible()
    await page.getByPlaceholder('Medikament suchen …').fill(drugName)
    await page
      .getByRole('option', { name: new RegExp(drugName) })
      .first()
      .click()

    // Found now, with what the Data Matrix carried.
    await expect(page.getByRole('heading', { name: drugName })).toBeVisible()
    await expect(page.getByLabel('Chargennummer')).toHaveValue('CH-NEU')
    await page.getByRole('button', { name: 'Stimmt' }).click()
    await page.getByRole('button', { name: 'Einbuchen' }).click()
    const booked = page.getByRole('listitem').filter({ hasText: drugName })
    await expect(booked).toContainText('CH-NEU')

    // The drug knows the code from now on.
    await login(request)
    const drug = (await (await request.get(`/api/drugs/${drugId}`)).json()) as {
      codes: { display: string }[]
    }
    expect(drug.codes.map((code) => code.display)).toContain(`PZN ${pzn}`)

    await booked.getByRole('button', { name: 'Rückgängig' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Rückgängig' }).click()
    await expect(booked).toHaveCount(0)
    const lots = (await (
      await request.get(`/api/lots?drug_id=${drugId}&empty=true`)
    ).json()) as unknown[]
    expect(lots).toHaveLength(0)
  })

  test('a new drug is made for an unknown code, and "Zurück" returns to the delivery', async ({
    page,
  }) => {
    const pzn = uniquePzn()
    await openDelivery(page)
    await scan(page, `-${pzn}`)
    await page.getByRole('button', { name: 'Neues Medikament' }).click()

    // The new drug, with the code already on it.
    await expect(page).toHaveURL(/\/pharmacy\/\d+$/)
    await expect(page.getByText(`PZN ${pzn}`)).toBeVisible()
    await page.getByRole('link', { name: 'Zurück: Lieferung einbuchen' }).click()

    // Back on the delivery, the code names the new drug — which has no original packaging yet.
    await expect(page).toHaveURL(/\/pharmacy\/delivery$/)
    await expect(page.getByRole('alert')).toContainText('noch keine vollständige Originalpackung')
  })
})
