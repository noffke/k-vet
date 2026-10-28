import { expect, test } from '@playwright/test'
import { E2E_PASSWORD, E2E_USERNAME } from '../fixtures/credentials'
import { seedCustomer, seedPatient } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

const basic = `Basic ${Buffer.from(`${E2E_USERNAME}:${E2E_PASSWORD}`).toString('base64')}`

/**
 * The appointments as a calendar on the phone (CalDAV at /dav, migration 0036): what the
 * phone's sync app sends is answered as a calendar server would, and an entry made on the
 * phone turns up as an appointment whose customer the vet picks — suggested, never assigned.
 */
test.describe('the calendar on the phone', () => {
  test('an entry made on the phone becomes an appointment, its customer suggested', async ({
    page,
    request,
  }) => {
    const { lastName } = await seedCustomer(request)
    const uid = `phone-${Date.now()}`
    const event = [
      'BEGIN:VCALENDAR',
      'VERSION:2.0',
      'PRODID:e2e',
      'BEGIN:VEVENT',
      `UID:${uid}`,
      'DTSTAMP:20261020T080000Z',
      'DTSTART;TZID=Europe/Berlin:20271104T100000',
      'DTEND;TZID=Europe/Berlin:20271104T110000',
      `SUMMARY:${lastName} Impfung`,
      'END:VEVENT',
      'END:VCALENDAR',
      '',
    ].join('\r\n')
    const put = await request.fetch(`/dav/calendars/praxis/${uid}.ics`, {
      method: 'PUT',
      headers: { Authorization: basic, 'Content-Type': 'text/calendar', 'If-None-Match': '*' },
      data: event,
    })
    expect(put.status()).toBe(201)

    // The sync app sees it, under its own name.
    const listing = await request.fetch('/dav/calendars/praxis/', {
      method: 'PROPFIND',
      headers: { Authorization: basic, Depth: '1' },
      data: '<propfind xmlns="DAV:"><prop><getetag/></prop></propfind>',
    })
    expect(listing.status()).toBe(207)
    expect(await listing.text()).toContain(`${uid}.ics`)

    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByPlaceholder('Suchen …').fill(`${lastName} Impfung`)
    await expect(page.getByText('Aus Kalender').filter({ visible: true }).first()).toBeVisible()
    await page.getByText('Aus Kalender').filter({ visible: true }).first().click()

    await expect(page.getByText(`„${lastName} Impfung“`)).toBeVisible()
    await page.getByRole('button', { name: new RegExp(`Erika ${lastName}`) }).click()
    await expect(page.getByText('Aus Kalender')).toHaveCount(0)
    await expect(page.getByText(new RegExp(`Erika ${lastName}`)).first()).toBeVisible()
  })

  test('Einstellungen says where to point the phone', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Einstellungen')
    const card = page.locator('section').filter({ hasText: 'Kalender auf dem Handy' })
    await expect(card).toContainText('/dav/')
    await expect(card).toContainText(E2E_USERNAME)
  })
})
