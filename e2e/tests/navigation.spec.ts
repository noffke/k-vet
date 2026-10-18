import { expect, test } from '@playwright/test'
import { seedCustomer, seedPatient, seedTreatment } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * "Zurück" goes back to where the vet came from (issues.md): customer → animal → appointment
 * used to lead back to all appointments. It now retraces the way, naming each page — and a
 * page opened from nowhere in this tab still offers its own parent.
 */
test.describe('navigation', () => {
  test('"Zurück" retraces customer → animal → appointment', async ({ page, request }) => {
    const { customerId, lastName } = await seedCustomer(request)
    const name = `Lotta-${Date.now() % 1e6}`
    const patientId = await seedPatient(request, customerId, name)
    const { appointmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/customers/${customerId}`)
    await page.getByRole('button', { name: new RegExp(name) }).click()
    await expect(page).toHaveURL(new RegExp(`/patients/${patientId}$`))

    // The visit history opens the appointment on this animal's tab.
    await page.getByRole('link', { name: /\d{2}\.\d{2}\.\d{4}/ }).first().click()
    await expect(page).toHaveURL(new RegExp(`/appointments/${appointmentId}\\?tier=`))

    await page.getByRole('link', { name: `Zurück: ${name}` }).click()
    await expect(page).toHaveURL(new RegExp(`/patients/${patientId}$`))

    await page.getByRole('link', { name: new RegExp(`^Zurück: .*${lastName}`) }).click()
    await expect(page).toHaveURL(new RegExp(`/customers/${customerId}$`))
  })

  test('a page opened directly offers its own parent', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    const { appointmentId } = await seedTreatment(request, patientId)

    await signIn(page)
    await page.goto(`/appointments/${appointmentId}`)
    await page.getByRole('link', { name: 'Zurück: Termine' }).click()
    await expect(page).toHaveURL(/\/appointments$/)
  })
})
