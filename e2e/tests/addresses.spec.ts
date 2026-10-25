import { expect, test } from '@playwright/test'
import { MISSPELLED_STREET, UNKNOWN_HOUSE } from '../fixtures/fakeGoogle'
import { seedCustomer, seedPatient, seedTreatment } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * A customer's address: checked against Google once typed — a hint, never a block — and a
 * link that opens the drive there in Google Maps, from the customer and from the appointment.
 */
test.describe('customer addresses', () => {
  const drive = 'destination=Musterweg%205%2C%2012345%20Musterstadt'

  test('are checked once typed, and a correction can be taken over', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page)
    await page.goto(`/customers/${customerId}`)
    const street = page.getByLabel('Straße und Hausnummer')

    // Opening a customer asks nothing.
    await expect(page.getByText('Adresse gefunden')).toHaveCount(0)

    await street.fill(`${MISSPELLED_STREET} 5`)
    // Still typing in the address: not yet.
    await page.getByLabel('PLZ').click()
    await expect(page.getByText(/Meinten Sie/)).toHaveCount(0)
    await page.getByLabel('Telefon').click()
    await expect(page.getByText('Meinten Sie: Musterweg 5, 12345 Musterstadt?')).toBeVisible()
    await page.getByRole('button', { name: 'Übernehmen' }).click()
    await expect(street).toHaveValue('Musterweg 5')
    await expect(page.getByText('Adresse gefunden')).toBeVisible()

    // A house Google does not know is said, and saved all the same.
    await street.fill(`Musterweg ${UNKNOWN_HOUSE}`)
    await page.getByLabel('Telefon').click()
    await expect(page.getByText('Adresse nicht gefunden — bitte prüfen.')).toBeVisible()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await page.reload()
    await expect(street).toHaveValue(`Musterweg ${UNKNOWN_HOUSE}`)
  })

  test('open the drive there in Google Maps', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const { treatmentId } = await seedTreatment(request, await seedPatient(request, customerId))
    await signIn(page)

    await page.goto(`/customers/${customerId}`)
    const fromCustomer = page.getByRole('link', { name: 'In Google Maps öffnen' })
    await expect(fromCustomer).toHaveAttribute('href', new RegExp(drive))
    await expect(fromCustomer).toHaveAttribute('target', '_blank')

    // The appointment shows where the house call goes.
    await page.goto(`/treatments/${treatmentId}`)
    const fromVisit = page.getByRole('link', { name: 'Musterweg 5, 12345 Musterstadt' })
    await expect(fromVisit).toHaveAttribute('href', new RegExp(drive))
    await expect(fromVisit).toHaveAttribute('target', '_blank')
  })
})
