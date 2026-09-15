import { expect, test } from '@playwright/test'
import { navigate, signIn } from './helpers'

/**
 * Hersteller and Lieferanten (issues.md): until this screen existed they could only be
 * created by calling the API, so the two dropdowns in the pharmacy could never be filled
 * from the app at all.
 */
test.describe('master data', () => {
  test('a manufacturer is created here and can then be picked on a drug', async ({ page }) => {
    const name = `Pharma-${Date.now().toString().slice(-6)}`
    await signIn(page)
    await navigate(page, 'Stammdaten')

    await page.getByRole('button', { name: 'Neuer Hersteller' }).click()
    await expect(page).toHaveURL(/\/masterdata\/manufacturers\/\d+$/)
    // A fresh entry is a draft until it has a name.
    await expect(page.getByText('Unvollständig')).toBeVisible()

    await page.getByLabel('Name').fill(name)
    await page.getByLabel('Straße und Hausnummer').fill('Industrieweg 3')
    await page.getByLabel('PLZ').fill('40213')
    await page.getByLabel('Ort').fill('Düsseldorf')
    await page.getByLabel('Ort').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Unvollständig')).toHaveCount(0)

    await page.getByRole('link', { name: 'Zurück: Stammdaten' }).click()
    await expect(page.getByText(name).filter({ visible: true }).first()).toBeVisible()

    // The point of the screen: the new entry is selectable where drugs are entered.
    await navigate(page, 'Apotheke')
    await page.getByRole('button', { name: 'Neues Medikament' }).click()
    await expect(page.getByLabel('Hersteller')).toContainText(name)
  })

  test('both address books line their columns up', async ({ page }, testInfo) => {
    // Cards on a phone, so there is nothing to align there.
    test.skip(testInfo.project.name !== 'desktop', 'desktop layout only')
    const long = `Boehringer-${Date.now().toString().slice(-6)}`
    await signIn(page)
    await navigate(page, 'Stammdaten')

    // Names and places of different lengths: each table would otherwise size its own
    // columns to its own content and the two Ort columns would start at different places.
    await page.getByRole('button', { name: 'Neuer Hersteller' }).click()
    await page.getByLabel('Name').fill(long)
    await page.getByLabel('Ort').fill('Ingelheim am Rhein')
    await page.getByLabel('Ort').blur()
    await page.getByRole('link', { name: 'Zurück: Stammdaten' }).click()

    await page.getByRole('button', { name: 'Neuer Lieferant' }).click()
    await page.getByLabel('Name').fill('Kurz')
    await page.getByLabel('Ort').fill('Ulm')
    await page.getByLabel('Ort').blur()
    await page.getByRole('link', { name: 'Zurück: Stammdaten' }).click()
    await expect(page.getByText('Ulm').filter({ visible: true }).first()).toBeVisible()

    const lefts = await page
      .getByRole('columnheader', { name: 'Ort' })
      .evaluateAll((headers) => headers.map((header) => header.getBoundingClientRect().left))
    expect(lefts).toHaveLength(2)
    expect(lefts[0]).toBe(lefts[1])
  })

  test('a supplier can be archived and comes back when asked for', async ({ page }) => {
    const name = `Depot-${Date.now().toString().slice(-6)}`
    await signIn(page)
    await navigate(page, 'Stammdaten')

    await page.getByRole('button', { name: 'Neuer Lieferant' }).click()
    await page.getByLabel('Name').fill(name)
    await page.getByLabel('Name').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Archiving asks first now (issues.md 2), and names the record so it is obvious which
    // one is about to leave the lists. Both the trigger and the confirm are called
    // "Archivieren", so the confirm has to be taken from inside the dialog.
    await page.getByRole('button', { name: 'Archivieren' }).click()
    const confirm = page.getByRole('dialog')
    await expect(confirm.getByText(new RegExp(name))).toBeVisible()
    await confirm.getByRole('button', { name: 'Archivieren' }).click()
    await expect(page.getByText('Archiviert').first()).toBeVisible()

    await page.getByRole('link', { name: 'Zurück: Stammdaten' }).click()
    await expect(page.getByText(name).filter({ visible: true })).toHaveCount(0)

    await page.getByLabel('Archivierte anzeigen').check()
    await expect(page.getByText(name).filter({ visible: true }).first()).toBeVisible()
  })
})
