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

  test('a supplier can be archived and comes back when asked for', async ({ page }) => {
    const name = `Depot-${Date.now().toString().slice(-6)}`
    await signIn(page)
    await navigate(page, 'Stammdaten')

    await page.getByRole('button', { name: 'Neuer Lieferant' }).click()
    await page.getByLabel('Name').fill(name)
    await page.getByLabel('Name').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await page.getByRole('button', { name: 'Archivieren' }).click()
    await expect(page.getByText('Archiviert').first()).toBeVisible()

    await page.getByRole('link', { name: 'Zurück: Stammdaten' }).click()
    await expect(page.getByText(name).filter({ visible: true })).toHaveCount(0)

    await page.getByLabel('Archivierte anzeigen').check()
    await expect(page.getByText(name).filter({ visible: true }).first()).toBeVisible()
  })
})
