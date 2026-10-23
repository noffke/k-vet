import { expect, test } from '@playwright/test'
import { seedAcceptedInvoice, seedFailedMail } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * Getting a released invoice to the customer by e-mail. No request talks to the mail server
 * (migration 0026): sending queues the mail, the queue's run delivers it every five minutes, and
 * what failed waits on the dashboard to be retried or removed.
 */
test.describe('sending an invoice', () => {
  test('names each address by type and queues the mail', async ({ page, request }) => {
    const invoice = await seedAcceptedInvoice(request)
    await signIn(page)
    await page.goto(`/treatments/${invoice.treatmentId}/invoice/send`)
    await expect(page.getByText('erika@example.com').first()).toContainText('privat')

    await page.getByRole('button', { name: 'E-Mail senden', exact: true }).click()
    await expect(page.getByText('Die E-Mail wird in den nächsten Minuten versendet.')).toBeVisible()
    // Back on the appointment, which says where the mail is.
    await expect(page).toHaveURL(/\/appointments\/\d+/)
    await expect(
      page.getByText(/E-Mail an erika@example\.com wartet auf den Versand/),
    ).toBeVisible()
    await expect(page.getByText('Nicht versendet', { exact: true })).toBeVisible()
  })

  test('a new address typed when sending again is offered to the customer', async ({
    page,
    request,
  }) => {
    const invoice = await seedAcceptedInvoice(request)
    const address = `neu-${invoice.invoiceId}@example.com`
    await signIn(page)
    await page.goto(`/treatments/${invoice.treatmentId}/invoice/send`)

    // The same page as releasing: one more address, and the question whether to keep it.
    await page.getByLabel('Weitere E-Mail-Adresse').fill(address)
    await page.getByRole('button', { name: 'E-Mail senden', exact: true }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText(address)
    await dialog.getByRole('button', { name: 'Nur diesmal verwenden' }).click()

    await expect(page.getByText(`E-Mail an erika@example.com, ${address} wartet`)).toBeVisible()
  })

  test('a failed mail waits on the dashboard to be retried or removed', async ({
    page,
    request,
  }) => {
    const retried = await seedAcceptedInvoice(request)
    const removed = await seedAcceptedInvoice(request)
    await seedFailedMail(retried.invoiceId, `alt-${retried.invoiceId}@example.com`)
    await seedFailedMail(removed.invoiceId, `weg-${removed.invoiceId}@example.com`)

    await signIn(page)
    await page.goto('/')
    const card = page.locator('section').filter({ hasText: 'E-Mail-Versand fehlgeschlagen' })
    const row = (invoiceNumber: string) =>
      card.getByRole('listitem').filter({ hasText: invoiceNumber })

    // What the mail server said, and that it will not be asked again by itself.
    await expect(row(retried.invoiceNumber)).toContainText('Abgelehnt — kein weiterer Versuch')
    await expect(row(retried.invoiceNumber)).toContainText('550')

    // The appointment says so too, and leads here.
    await row(retried.invoiceNumber).getByRole('link').click()
    await expect(page.getByText(/E-Mail-Versand abgebrochen: permanent error \(550\)/)).toBeVisible()
    await page.goto('/')

    await row(retried.invoiceNumber).getByRole('button', { name: 'Erneut versuchen' }).click()
    await expect(page.getByText(/mit dem nächsten Durchlauf erneut versendet/)).toBeVisible()
    await expect(row(retried.invoiceNumber)).toHaveCount(0)

    await row(removed.invoiceNumber)
      .getByRole('button', { name: 'Aus der Warteschlange entfernen' })
      .click()
    await expect(row(removed.invoiceNumber)).toHaveCount(0)

    // Retried, the mail waits for the next run; removed, the invoice is simply not sent.
    await page.goto(`/treatments/${retried.treatmentId}`)
    await expect(page.getByText(/wartet auf den Versand/)).toBeVisible()
    await page.goto(`/treatments/${removed.treatmentId}`)
    await expect(page.getByText('Nicht versendet', { exact: true })).toBeVisible()
    await expect(page.getByText(/wartet auf den Versand|E-Mail-Versand abgebrochen/)).toHaveCount(0)
  })
})
