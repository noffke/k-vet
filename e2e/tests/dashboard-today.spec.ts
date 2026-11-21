import { seedCustomer, seedPatient } from '../fixtures/seed'
import { expect, test } from '../fixtures/test'
import { signIn } from './helpers'

/**
 * The dashboard lists today's appointments: each opens its appointment, and carries the address
 * with the drive there in Google Maps — the Maps app on the phone during a house call.
 */
test.describe('the dashboard', () => {
  test("lists today's appointments, each opening it, with the route to it", async ({
    page,
    request,
  }) => {
    const { customerId, lastName } = await seedCustomer(request)
    const animal = `Heute-${Date.now() % 1e6}`
    await seedPatient(request, customerId, animal)
    // Early in the day, so it is at the top of a day the worker's database has filled.
    const early = new Date()
    early.setHours(0, 5, 0, 0)
    const created = await request.post('/api/appointments', {
      data: { starts_at: early.toISOString(), customer_id: customerId },
    })
    const { id } = (await created.json()) as { id: number }

    await signIn(page)
    const card = page
      .locator('section')
      .filter({ has: page.getByRole('heading', { name: /^Heute/ }) })
    const row = card.getByRole('listitem').filter({ hasText: lastName })
    await expect(row).toContainText(animal)
    await expect(row).toContainText('Musterweg 5, 12345 Musterstadt')
    await expect(row.getByRole('link', { name: 'Route', exact: true })).toHaveAttribute(
      'href',
      /^https:\/\/www\.google\.com\/maps\/dir\/\?api=1&destination=Musterweg%205/,
    )

    await row.getByRole('link').first().click()
    await expect(page).toHaveURL(new RegExp(`/appointments/${id}`))
  })
})
