import { expect, test } from '@playwright/test'
import { seedAcceptedInvoice } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * Getting a released invoice to the customer by e-mail — and what the screen says when the
 * practice's mail server does not answer, which on an appliance behind a practice network is
 * the failure that actually happens.
 */
test.describe('sending an invoice', () => {
  test('names each address by type and explains a failed send', async ({ page, request }) => {
    const invoice = await seedAcceptedInvoice(request)
    await signIn(page)
    await page.goto(`/treatments/${invoice.treatmentId}/invoice/send`)

    // Which of several addresses is the private one is the decision being made here.
    await expect(page.getByText('erika@example.com').first()).toContainText('privat')

    // No SMTP server is reachable from the suite, so this is the real failure path.
    await page.getByRole('button', { name: 'E-Mail senden', exact: true }).click()

    // It used to say "Nicht gespeichert", which named the wrong thing and offered no way on.
    const alert = page.getByRole('alert')
    await expect(alert).toBeVisible()
    await expect(alert).toContainText('Mailserver ist nicht erreichbar')
    await expect(alert).toContainText('Per Post versendet')
    await expect(alert).not.toContainText('Nicht gespeichert')

    // The invoice is untouched, so the send can be retried once the relay is fixed.
    await expect(page).toHaveURL(/\/invoice\/send$/)
  })
})
