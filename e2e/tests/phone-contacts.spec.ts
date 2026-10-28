import { expect, test } from '@playwright/test'
import { E2E_PASSWORD, E2E_USERNAME } from '../fixtures/credentials'
import { seedCustomer, seedPatient } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

const basic = `Basic ${Buffer.from(`${E2E_USERNAME}:${E2E_PASSWORD}`).toString('base64')}`

/**
 * The customers as contacts on the phone (CardDAV at /dav, migration 0037): a customer is a
 * contact named with the animals, and a contact made on the phone turns up as a customer "Vom
 * Handy" that the vet completes — with its second number in "Mobil".
 */
test.describe('the contacts on the phone', () => {
  test('a customer is a contact named with the animals', async ({ request }) => {
    const { customerId, lastName } = await seedCustomer(request)
    await seedPatient(request, customerId, 'Bello')
    await seedPatient(request, customerId, 'Minka')

    const contact = await request.fetch(
      `/dav/addressbooks/kunden/kvet-customer-${customerId}.vcf`,
      { headers: { Authorization: basic } },
    )
    expect(contact.status()).toBe(200)
    const vcf = (await contact.text()).replaceAll('\r\n ', '')
    expect(vcf).toContain(`FN:Erika ${lastName} (Bello\\, Minka)`)
    expect(vcf).toContain('Bello (Hund)')
  })

  test('a contact made on the phone becomes a customer to complete', async ({ page, request }) => {
    const lastName = `Handy-${Date.now().toString().slice(-6)}`
    const uid = `contact-${Date.now()}`
    const card = [
      'BEGIN:VCARD',
      'VERSION:3.0',
      `UID:${uid}`,
      `FN:Heike ${lastName}`,
      `N:${lastName};Heike;;;`,
      'TEL;TYPE=CELL:0171 7654321',
      'ADR;TYPE=HOME:;;Dorfstraße 1;Neustadt;;54321;',
      'END:VCARD',
      '',
    ].join('\r\n')
    const put = await request.fetch(`/dav/addressbooks/kunden/${uid}.vcf`, {
      method: 'PUT',
      headers: { Authorization: basic, 'Content-Type': 'text/vcard', 'If-None-Match': '*' },
      data: card,
    })
    expect(put.status()).toBe(201)

    await signIn(page)
    await navigate(page, 'Kunden')
    await page.getByPlaceholder('Suchen …').fill(lastName)
    await expect(page.getByText('Vom Handy').filter({ visible: true }).first()).toBeVisible()
    await page.getByText(`Heike ${lastName}`).filter({ visible: true }).first().click()

    await expect(page.getByLabel('Mobil')).toHaveValue(/^0171/)
    await expect(page.getByLabel('Telefon')).toHaveValue('')
    await expect(page.getByTitle('Auf dem Handy als Kontakt angelegt')).toBeVisible()

    // The salutation is all a contact with an address lacks: then it is a customer like any.
    await page.getByLabel('Anrede').first().selectOption('frau')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByTitle(/Unvollständig/)).toHaveCount(0)
    await expect(page.getByText('Vom Handy')).toHaveCount(0)

    // A second number typed here goes to the phone with the next sync.
    await page.getByLabel('Mobil').fill('0172 1112223')
    await page.getByLabel('Mobil').blur()
    await expect
      .poll(async () => {
        const served = await request.fetch(`/dav/addressbooks/kunden/${uid}.vcf`, {
          headers: { Authorization: basic },
        })
        return served.text()
      })
      .toContain('TEL;TYPE=CELL:+491721112223')
  })
})
