import { expect, test } from '@playwright/test'
import { germanDate } from '../fixtures/clock'
import { seedCustomer, seedDrug, seedPatient, seedTreatment } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * "Mitbringen" (migration 0035): a customer phones for more of a regular medication; it goes on
 * the appointment instead of a slip of paper, shows on the day's Packliste, is ticked off while
 * packing, and becomes a position at the visit.
 */
test.describe('Mitbringen', () => {
  test('a drug to bring is on the Packliste, packed, and taken into a position', async ({
    page,
    request,
  }) => {
    const { customerId, lastName } = await seedCustomer(request)
    const animal = `Rex-${Date.now() % 1e6}`
    const patientId = await seedPatient(request, customerId, animal)
    const { appointmentId } = await seedTreatment(request, patientId)
    const { drugName } = await seedDrug(request)

    await signIn(page)
    await page.goto(`/appointments/${appointmentId}`)
    await page.getByPlaceholder('Medikament suchen …').fill(drugName)
    await page.getByRole('option', { name: /· 10 ml/ }).first().click()
    const card = page.locator('section').filter({ hasText: 'Mitbringen' }).first()
    const row = card.getByRole('listitem').filter({ has: page.getByLabel('Notiz') })
    await expect(row).toContainText(`${drugName} · 10 ml`)
    await row.getByLabel('Notiz').fill('für den Rücken')
    await row.getByLabel('Notiz').blur()
    // Written down, not dispensed: no position yet.
    await expect(page.getByRole('tabpanel').getByLabel('Name')).toHaveCount(0)

    // The day's Packliste has it, added up and visit by visit; a tick survives a reload.
    await card.getByRole('link', { name: 'Packliste' }).click()
    await expect(page.getByText(`Was für ${germanDate()} mitzunehmen ist`)).toBeVisible()
    await expect(page.getByText(new RegExp(`1 × ${drugName}`)).first()).toBeVisible()
    await expect(page.getByText(lastName).filter({ visible: true }).first()).toBeVisible()
    const packed = page.getByRole('checkbox', { name: new RegExp(drugName) })
    await packed.check()
    await expect(packed).toBeChecked()
    await page.reload()
    await expect(packed).toBeChecked()
    await expect(page.getByText('für den Rücken').first()).toBeVisible()

    // The overview's own entry leads there too.
    await navigate(page, 'Packliste')
    await expect(page.getByRole('heading', { name: 'Packliste' })).toBeVisible()

    // At the visit: one click, and it is the animal's position, dispensed like a picked drug.
    await page.goto(`/appointments/${appointmentId}`)
    await row.getByRole('button', { name: 'Als Position übernehmen' }).click()
    await expect(page.getByText(`„${drugName}“ als Position übernommen`)).toBeVisible()
    await expect(card.getByText('Übernommen', { exact: true })).toBeVisible()
    await expect(page.getByRole('tabpanel').getByLabel('Name')).toHaveValue(new RegExp(drugName))
  })
})
