import { expect, test } from '@playwright/test'
import {
  clearPendingInvoices,
  seedAcceptedInvoice,
  seedDrug,
  seedSentInvoice,
} from '../fixtures/seed'
import { hasColourNearTop, navigate, pdfFirstPage, pdfText, signIn } from './helpers'

/**
 * Dashboard widgets and practice settings (T075): each widget leads to the work it
 * announces, and a changed IBAN is on the next invoice PDF.
 */
test.describe('dashboard and settings', () => {
  test('the widgets lead to the work they announce', async ({ page, request }) => {
    await clearPendingInvoices(request)
    await seedSentInvoice(request)
    await seedSentInvoice(request)
    // Released and stuck here — the case that used to be invisible.
    const unsent = await seedAcceptedInvoice(request)

    // A lot that expires next week, so it belongs in the expiring list.
    const { drugName, packagingId } = await seedDrug(request)
    const expiresOn = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10)
    // Unique per run: the specs share one database.
    const batchNumber = `CH-VERFALL-${Date.now().toString().slice(-6)}`
    const intake = await request.post(`/api/packagings/${packagingId}/stock-intakes`, {
      data: { packages_received: 1, batch_number: batchNumber, expiration_date: expiresOn },
    })
    expect(intake.ok()).toBe(true)
    const lotId = (await intake.json()).id as number

    await signIn(page)

    // The count is a door, not a decoration.
    const pending = page.getByRole('link', { name: /Rechnungen für die Buchhaltung/ })
    await expect(pending).toContainText('2')
    await pending.click()
    await expect(page).toHaveURL(/\/invoices$/)

    // So is the money that never left the practice.
    await navigate(page, 'Übersicht')
    const unsentCard = page.locator('section').filter({ hasText: 'Nicht versendet' }).first()
    await expect(unsentCard.getByText('1', { exact: true })).toBeVisible()
    await unsentCard.getByRole('link').first().click()
    await expect(page).toHaveURL(new RegExp(`/treatments/${unsent.treatmentId}$`))

    await navigate(page, 'Übersicht')
    await expect(page.getByText(batchNumber, { exact: false })).toBeVisible()
    await page.getByRole('link', { name: new RegExp(drugName) }).click()
    await expect(page).toHaveURL(new RegExp(`/lots/${lotId}$`))
  })

  test('the practice and bank details are on the next invoice PDF', async ({ page, request }) => {
    const iban = `DE${Date.now().toString().slice(-18)}`
    const practiceName = `Tierarztpraxis ${Date.now().toString().slice(-6)}`
    const practiceEmail = `praxis-${Date.now().toString().slice(-6)}@example.com`
    const bic = 'BYLADEM1001'

    await signIn(page)
    await navigate(page, 'Einstellungen')

    await page.getByLabel('Name der Praxis').fill(practiceName)
    await page.getByLabel('Straße und Hausnummer').fill('Dorfstraße 1')
    await page.getByLabel('PLZ').fill('12345')
    await page.getByLabel('Ort').fill('Musterstadt')
    await page.getByLabel('E-Mail der Praxis').fill(practiceEmail)
    await page.getByLabel('IBAN').fill(iban)
    await page.getByLabel('BIC').fill(bic)
    await page.getByLabel('BIC').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // The invoice is written after the change, so it must carry the new values.
    const invoice = await seedAcceptedInvoice(request)
    const response = await page.request.get(`/api/invoices/${invoice.invoiceId}/pdf`)
    expect(response.status()).toBe(200)
    const text = await pdfText(Buffer.from(await response.body()))

    expect(text).toContain(iban)
    expect(text).toContain(practiceName)
    expect(text).toContain(practiceEmail)
    expect(text).toContain(bic)
    expect(text).toContain(invoice.invoiceNumber)

    // The due date carries the whole reason it exists: lexoffice should read it off the PDF
    // instead of the vet retyping it. Assert the label, not just some date.
    expect(text).toContain('Fälligkeitsdatum:')

    // The GiroCode is an image, so pdftotext cannot see it — assert its caption instead.
    expect(text).toContain('GiroCode')
  })

  test('a malformed copy address is refused with a field error', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Einstellungen')

    const cc = page.getByLabel('CC-Adressen', { exact: true })
    await cc.fill('kein-at-zeichen')
    await cc.blur()

    await expect(page.getByText('Keine gültige E-Mail-Adresse')).toBeVisible()

    // Correcting it saves, and the value survives a reload.
    await cc.fill('buchhaltung@example.com')
    await cc.blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    await page.reload()
    await expect(page.getByLabel('CC-Adressen', { exact: true })).toHaveValue(
      'buchhaltung@example.com',
    )
  })

  test('the practice logo prints whole, not cropped at the top of the sheet', async ({
    page,
    request,
  }) => {
    // Three bands. The bug printed only the bottom one, clipped at the paper edge, so asking
    // for the *top* band is what tells the two apart (issues.md 9).
    const logo = Buffer.from(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 200">
         <rect width="600" height="67" fill="rgb(220,60,40)"/>
         <rect y="67" width="600" height="66" fill="rgb(60,160,80)"/>
         <rect y="133" width="600" height="67" fill="rgb(40,80,200)"/>
       </svg>`,
    )

    await signIn(page)
    await navigate(page, 'Einstellungen')
    await page.locator('input[type=file]').setInputFiles({
      name: 'logo.svg',
      mimeType: 'image/svg+xml',
      buffer: logo,
    })
    await expect(page.getByRole('img', { name: 'Logo' })).toBeVisible()

    const invoice = await seedAcceptedInvoice(request)
    const response = await page.request.get(`/api/invoices/${invoice.invoiceId}/pdf`)
    expect(response.status()).toBe(200)
    const raster = await pdfFirstPage(Buffer.from(await response.body()))

    expect(hasColourNearTop(raster, [220, 60, 40]), 'the top band of the logo is on the page')
      .toBeTruthy()
    expect(hasColourNearTop(raster, [60, 160, 80]), 'the middle band too').toBeTruthy()
    expect(hasColourNearTop(raster, [40, 80, 200]), 'and the bottom band').toBeTruthy()

    // Put the practice back as it was, since the specs share one database and the rest of them
    // do not expect a letterhead.
    await page.getByRole('button', { name: 'Löschen' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen' }).click()
    await expect(page.getByText('Kein Logo hinterlegt')).toBeVisible()
  })

  test('the logo is uploaded, shown, and can be taken off again', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Einstellungen')

    // The file input is visually hidden by design, so the file goes in directly.
    await page.locator('input[type=file]').setInputFiles({
      name: 'logo.png',
      mimeType: 'image/png',
      buffer: Buffer.from(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGOYHaAMAAKYAQ/3EBc/AAAAAElFTkSuQmCC',
        'base64',
      ),
    })

    await expect(page.getByRole('img', { name: 'Logo' })).toBeVisible()

    // Taking the logo off asks first (issues.md 2). The trigger and the confirm share the
    // label, so the confirm is taken from inside the dialog.
    await page.getByRole('button', { name: 'Löschen' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Löschen' }).click()
    await expect(page.getByText('Kein Logo hinterlegt')).toBeVisible()
  })
})
