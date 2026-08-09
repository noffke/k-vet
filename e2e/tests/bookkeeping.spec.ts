import { expect, test } from '@playwright/test'
import { clearPendingInvoices, seedAcceptedInvoice, seedSentInvoice } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * The monthly bookkeeping hand-off (T070, quickstart #8): three accepted invoices are
 * downloaded as one bundle and marked as handed over in a single step.
 */
test.describe('bookkeeping hand-off', () => {
  test('three pending invoices are bundled, handed over, and leave nothing open', async ({
    page,
    request,
  }) => {
    // The specs share one database, so the slate is cleared before counting.
    await clearPendingInvoices(request)
    const invoices = [
      await seedSentInvoice(request),
      await seedSentInvoice(request),
      await seedSentInvoice(request),
    ]

    await signIn(page)
    await navigate(page, 'Rechnungen')

    const pending = page.locator('section').first()
    await expect(pending.getByText('3', { exact: true })).toBeVisible()
    for (const invoice of invoices) {
      await expect(
        pending.getByText(invoice.invoiceNumber).filter({ visible: true }).first(),
      ).toBeVisible()
    }

    // One click, one file: the bundle of every pending PDF.
    const [download] = await Promise.all([
      page.waitForEvent('download'),
      page.getByRole('link', { name: 'Alle PDFs herunterladen' }).click(),
    ])
    expect(download.suggestedFilename()).toMatch(/^Rechnungen-\d{4}-\d{2}-\d{2}\.zip$/)
    const stream = await download.createReadStream()
    const chunks: Buffer[] = []
    for await (const chunk of stream) chunks.push(chunk as Buffer)
    const bundle = Buffer.concat(chunks)
    expect(bundle.subarray(0, 2).toString('latin1')).toBe('PK')
    expect(bundle.byteLength).toBeGreaterThan(1000)

    // Downloading changes nothing; the hand-off is the separate, confirmed step.
    await expect(pending.getByText('3', { exact: true })).toBeVisible()

    await page
      .getByRole('button', { name: 'Alle als übergeben markieren' })
      .filter({ visible: true })
      .first()
      .click()
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Alle als übergeben markieren' })
      .click()

    await expect(page.getByText('Nichts offen — alles übergeben')).toBeVisible()
    await expect(pending.getByText('0', { exact: true })).toBeVisible()

    // The invoices are still listed below, now with the hand-off recorded.
    const all = page.locator('section').nth(1)
    for (const invoice of invoices) {
      const row = all.getByText(invoice.invoiceNumber).filter({ visible: true }).first()
      await expect(row).toBeVisible()
    }
    await expect(all.getByText('Übergeben').filter({ visible: true }).first()).toBeVisible()
  })

  test('an invoice is cancelled from the list and only shows up when asked for', async ({
    page,
    request,
  }) => {
    const invoice = await seedAcceptedInvoice(request)

    await signIn(page)
    await navigate(page, 'Rechnungen')

    const listed = () => page.getByText(invoice.invoiceNumber).filter({ visible: true })
    await expect(listed().first()).toBeVisible()

    await page
      .getByRole('button', { name: 'Rechnung stornieren' })
      .filter({ visible: true })
      .first()
      .click()
    await page.getByRole('button', { name: 'Bestätigen' }).click()

    await expect(listed()).toHaveCount(0)
    await page.getByLabel('Stornierte anzeigen').check()
    await expect(listed().first()).toBeVisible()
    await expect(page.getByText('Storniert').filter({ visible: true }).first()).toBeVisible()
  })

  test('the PDF of an invoice is reachable from the list', async ({ page, request }) => {
    const invoice = await seedAcceptedInvoice(request)

    await signIn(page)
    await page.goto('/invoices')

    const href = `/api/invoices/${invoice.invoiceId}/pdf`
    const link = page.getByRole('link', { name: 'PDF' }).filter({ visible: true })
    await expect(link.first()).toBeVisible()
    expect(await link.evaluateAll((nodes) => nodes.map((node) => node.getAttribute('href')))).toContain(
      href,
    )

    const pdf = await page.request.get(href)
    expect(pdf.status()).toBe(200)
    expect(pdf.headers()['content-type']).toBe('application/pdf')
  })
})
