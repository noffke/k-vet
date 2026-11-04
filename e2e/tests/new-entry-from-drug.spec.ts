import { expect, test } from '@playwright/test'
import { login, seedDrug } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

const stamp = () => Date.now().toString().slice(-6)

/**
 * A manufacturer or supplier made from the select on the drug that needs it: named in a small
 * dialog, set on the drug or packaging, opened for its address — and "Zurück" leads back to the
 * drug, which shows it chosen. Cancelling makes nothing.
 */
test.describe('a new manufacturer or supplier from the drug', () => {
  test('a manufacturer is made from the drug and chosen there on the way back', async ({
    page,
  }) => {
    const drugName = `Neuarznei-${stamp()}`
    const manufacturer = `Hersteller-${stamp()}`
    await signIn(page)
    await navigate(page, 'Apotheke')
    await page.getByRole('button', { name: 'Neues Medikament' }).click()
    await expect(page).toHaveURL(/\/pharmacy\/\d+$/)
    const drugUrl = page.url()
    await page.getByLabel('Name').fill(drugName)
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await page.getByLabel('Hersteller').selectOption({ label: 'Neuer Hersteller …' })
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText('Neuer Hersteller')
    await dialog.getByLabel('Name').fill(manufacturer)
    await dialog.getByRole('button', { name: 'Anlegen und öffnen' }).click()

    await expect(page).toHaveURL(/\/masterdata\/manufacturers\/\d+$/)
    await expect(page.getByLabel('Name')).toHaveValue(manufacturer)
    await page.getByRole('link', { name: `Zurück: ${drugName}` }).click()

    await expect(page).toHaveURL(drugUrl)
    await expect(page.getByLabel('Hersteller').locator('option:checked')).toHaveText(manufacturer)
  })

  test('a supplier is made from the original packaging', async ({ page, request }) => {
    const { drugId, drugName } = await seedDrug(request)
    const supplier = `Lieferant-${stamp()}`
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)

    await page.getByLabel('Lieferant').selectOption({ label: 'Neuer Lieferant …' })
    const dialog = page.getByRole('dialog')
    await dialog.getByLabel('Name').fill(supplier)
    await dialog.getByRole('button', { name: 'Anlegen und öffnen' }).click()

    await expect(page).toHaveURL(/\/masterdata\/suppliers\/\d+$/)
    await page.getByRole('link', { name: `Zurück: ${drugName}` }).click()
    await expect(page).toHaveURL(new RegExp(`/pharmacy/${drugId}$`))
    await expect(page.getByLabel('Lieferant').locator('option:checked')).toHaveText(supplier)
  })

  test('the choices show even when the lists arrive after the drug', async ({ page, request }) => {
    const { drugId } = await seedDrug(request)
    await signIn(page)
    // Hold the lists back, so the drug is there first. A select that took its default before
    // its options existed showed "—" for good (seen on a slow CI runner).
    for (const list of ['manufacturers', 'suppliers']) {
      await page.route(`**/api/${list}*`, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1500))
        await route.continue()
      })
    }
    await page.goto(`/pharmacy/${drugId}`)
    await expect(page.getByLabel('Hersteller').locator('option:checked')).toHaveText('Pharma AG')
    await expect(page.getByLabel('Lieferant').locator('option:checked')).toHaveText(
      'Großhandel GmbH',
    )
  })

  test('cancelling makes nothing and leaves the choice as it was', async ({ page, request }) => {
    const { drugId } = await seedDrug(request)
    const typed = `Abgebrochen-${stamp()}`
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)
    // Read the choice once the manufacturers are in: until then the select shows "—".
    await expect(page.getByLabel('Hersteller').locator('option:checked')).toHaveText('Pharma AG')
    const before = await page.getByLabel('Hersteller').inputValue()

    await page.getByLabel('Hersteller').selectOption({ label: 'Neuer Hersteller …' })
    const dialog = page.getByRole('dialog')
    await dialog.getByLabel('Name').fill(typed)
    await dialog.getByRole('button', { name: 'Abbrechen' }).click()

    await expect(dialog).toHaveCount(0)
    await expect(page).toHaveURL(new RegExp(`/pharmacy/${drugId}$`))
    await expect(page.getByLabel('Hersteller')).toHaveValue(before)
    await login(request)
    const found = (await (
      await request.get(`/api/manufacturers?q=${encodeURIComponent(typed)}`)
    ).json()) as unknown[]
    expect(found).toHaveLength(0)
  })
})
