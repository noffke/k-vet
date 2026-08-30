import { expect, test } from '@playwright/test'
import { seedAcceptedInvoice } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

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

/**
 * No screen may show a translation key (issues.md 18).
 *
 * The vitest suite checks every literal `t('…')` against the JSON, which is what caught the
 * invoice list's `FIELD.TOTAL`. This is the other half: keys that only arrive at runtime — the
 * message a backend field error carries, or a key built from a template literal — are invisible
 * to a static scan and only show up on a rendered page.
 */
test('no screen renders a translation key', async ({ page }) => {
  // `namespace.someKey` / `namespace.some_key`: a dotted identifier with no spaces, which is
  // what an unresolved key looks like and what ordinary prose never does.
  const KEY_SHAPED = /^[a-z][a-zA-Z]*\.[a-zA-Z][a-zA-Z0-9_.]*$/

  await signIn(page)
  const sections = [
    'Übersicht',
    'Termine',
    'Kunden',
    'Apotheke',
    'Leistungen',
    'Behandlungsgruppen',
    'Textbausteine',
    'Stammdaten',
    'Rechnungen',
    'Einstellungen',
  ]

  for (const section of sections) {
    await navigate(page, section)
    // Leaf elements only, so a container's concatenated text is not mistaken for a key.
    const leaked = await page
      .locator('body :not(:has(*))')
      .filter({ visible: true })
      .evaluateAll(
        (nodes, pattern) =>
          nodes
            .map((node) => (node.textContent ?? '').trim())
            .filter((text) => new RegExp(pattern).test(text)),
        KEY_SHAPED.source,
      )
    expect(leaked, `${section} renders a translation key`).toEqual([])
  }
})
