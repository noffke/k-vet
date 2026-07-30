import { expect, test } from '@playwright/test'
import { seedAcceptedInvoice } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * Locale spot check (T080): de-DE is the default, and switching to en-US changes numbers,
 * money and dates — in both directions, on a real screen.
 */
test('numbers, money and dates follow the active locale', async ({ page, request }) => {
  const invoice = await seedAcceptedInvoice(request)
  const today = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  const german = `${pad(today.getDate())}.${pad(today.getMonth() + 1)}.${today.getFullYear()}`
  const american = `${pad(today.getMonth() + 1)}/${pad(today.getDate())}/${today.getFullYear()}`

  await signIn(page)
  await page.goto('/invoices')

  const row = page
    .getByText(invoice.invoiceNumber)
    .filter({ visible: true })
    .first()
    .locator('xpath=ancestor::*[self::tr or self::li][1]')

  // de-DE: comma decimals, trailing currency symbol, dotted date.
  // 23,62 € net fee plus 19 % VAT — the list shows what the customer pays.
  await expect(row).toContainText('28,11 €')
  await expect(row).toContainText(german)

  const switchTo = (locale: string) =>
    page.getByRole('button', { name: locale, exact: true }).filter({ visible: true }).click()

  await switchTo('en')
  await expect(page.getByRole('link', { name: 'Invoices' }).first()).toBeVisible()

  // en-US: leading symbol, dot decimals, slashed date.
  await expect(row).toContainText('€28.11')
  await expect(row).toContainText(american)

  await switchTo('de')
  await expect(row).toContainText('28,11 €')
})
