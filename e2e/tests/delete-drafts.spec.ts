import { expect, test } from '@playwright/test'
import { login, seedDrug } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Incomplete records can be deleted: the one a stray "Neu" left behind goes again, asked first.
 * A complete record offers Archivieren instead, and one something still refers to stays — and
 * the screen says why.
 */
test.describe('deleting incomplete records', () => {
  test('a drug made by mistake is deleted, asked first', async ({ page, request }) => {
    await signIn(page)
    await navigate(page, 'Apotheke')
    await page.getByRole('button', { name: 'Neues Medikament' }).click()
    await expect(page).toHaveURL(/\/pharmacy\/\d+$/)
    const drugId = page.url().split('/').pop()

    // Incomplete: Löschen in place of Archivieren.
    await expect(page.getByRole('button', { name: 'Archivieren' })).toHaveCount(0)

    // A dismissed dialog leaves it alone.
    await page.getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(page.getByRole('dialog')).toContainText('wird endgültig gelöscht')
    await page.getByRole('dialog').getByRole('button', { name: 'Abbrechen' }).click()
    await expect(page).toHaveURL(new RegExp(`/pharmacy/${drugId}$`))

    await page.getByRole('button', { name: 'Löschen', exact: true }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen', exact: true }).click()
    // Back where it was started from.
    await expect(page).toHaveURL(/\/pharmacy$/)

    await login(request)
    expect((await request.get(`/api/drugs/${drugId}`)).status()).toBe(404)
  })

  test('a complete drug is archived, and a packaging added by mistake goes again', async ({
    page,
    request,
  }) => {
    const { drugId } = await seedDrug(request)
    await signIn(page)
    await page.goto(`/pharmacy/${drugId}`)

    await expect(page.getByRole('button', { name: 'Archivieren' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Löschen', exact: true })).toHaveCount(0)

    const subsets = page.getByRole('button', { name: 'Packung löschen' })
    await expect(subsets).toHaveCount(0)
    await page.getByRole('button', { name: 'Teilmenge' }).click()
    await expect(subsets).toHaveCount(1)
    await subsets.click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(subsets).toHaveCount(0)
    // The drug itself stays.
    await expect(page).toHaveURL(new RegExp(`/pharmacy/${drugId}$`))
  })

  test('a customer made by mistake is deleted; one with an animal stays and says why', async ({
    page,
    request,
  }) => {
    await signIn(page)
    await navigate(page, 'Kunden')
    await page.getByRole('button', { name: 'Neuer Kunde' }).click()
    await expect(page).toHaveURL(/\/customers\/\d+$/)
    const lastName = `Irrtum-${Date.now().toString().slice(-6)}`
    await page.getByLabel('Nachname').first().fill(lastName)
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await page.getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(page.getByRole('dialog')).toContainText(lastName)
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(page).toHaveURL(/\/customers$/)
    await page.getByPlaceholder('Suchen …').fill(lastName)
    await expect(page.getByText(lastName).filter({ visible: true })).toHaveCount(0)

    // Still incomplete, but an animal is on it: it stays, and the screen says so.
    await login(request)
    const customer = (await (await request.post('/api/customers')).json()) as { id: number }
    await request.post('/api/patients', { data: { customer_id: customer.id } })
    await page.goto(`/customers/${customer.id}`)
    await page.getByRole('button', { name: 'Löschen', exact: true }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen', exact: true }).click()
    await expect(page.getByRole('dialog')).toContainText('wird noch verwendet')
    await page.getByRole('dialog').getByRole('button', { name: 'Schließen', exact: true }).click()
    await expect(page).toHaveURL(new RegExp(`/customers/${customer.id}$`))
  })
})
