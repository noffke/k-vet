import { expect, test } from '@playwright/test'
import { seedCustomer, seedPatient } from '../fixtures/seed'
import { navigate, signIn } from './helpers'

/**
 * Master data (quickstart #11): warnings are impossible to miss, invalid contact data is
 * refused with a field message, and archived records stay out of the way until asked for.
 */
test.describe('customers and patients', () => {
  test('a new customer is created empty, completes while typing and is searchable', async ({
    page,
  }) => {
    await signIn(page)
    await navigate(page, 'Kunden')
    await page.getByRole('button', { name: 'Neuer Kunde' }).click()

    // The record exists from the first keystroke, flagged incomplete until it is whole.
    await expect(page.getByTitle(/Unvollständig/)).toBeVisible()

    await page.getByLabel('Anrede').first().selectOption('frau')
    await page.getByLabel('Vorname').first().fill('Erika')
    await page.getByLabel('Nachname').first().fill('Beispiel')
    await page.getByLabel('Straße und Hausnummer').fill('Musterweg 5')
    await page.getByLabel('PLZ').fill('12345')
    await page.getByLabel('Ort').fill('Musterstadt')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Filling the last mandatory field completes the record — no confirm step.
    await expect(page.getByTitle(/Unvollständig/)).toHaveCount(0)

    await navigate(page, 'Kunden')
    await page.getByPlaceholder('Suchen …').fill('Beispiel')
    await expect(page.getByText('Erika Beispiel').filter({ visible: true }).first()).toBeVisible()
  })

  test('an invalid email address is rejected with a field message', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page)
    await page.goto(`/customers/${customerId}`)

    await page.getByLabel('E-Mail').last().fill('erika(at)example.com')
    await page.getByRole('button', { name: 'E-Mail hinzufügen' }).click()

    await expect(page.getByText('Keine gültige E-Mail-Adresse')).toBeVisible()
  })

  test('a phone number is redisplayed in the national format', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page)
    await page.goto(`/customers/${customerId}`)

    await page.getByLabel('Telefon').fill('0171 / 123 45 67')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await page.reload()

    await expect(page.getByLabel('Telefon')).toHaveValue(/^0171/)
  })

  test('a warning remark is shown as a banner and as a list indicator', async ({
    page,
    request,
  }) => {
    const { customerId, lastName } = await seedCustomer(request)
    await signIn(page)
    await page.goto(`/customers/${customerId}`)

    await page.getByLabel('Warnhinweis').fill('Hund beißt bei der Blutabnahme')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Prominent on the record ...
    await expect(page.getByRole('alert')).toContainText('Hund beißt bei der Blutabnahme')

    // ... and an icon with the text as tooltip in the list.
    await navigate(page, 'Kunden')
    await page.getByPlaceholder('Suchen …').fill(lastName)
    await expect(
      page
        .getByLabel('Warnhinweis: Hund beißt bei der Blutabnahme')
        .filter({ visible: true })
        .first(),
    ).toBeVisible()
  })

  test('a weight is stored to one decimal and survives a reload', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId)
    await signIn(page)
    await page.goto(`/patients/${patientId}`)

    const weight = page.getByLabel('Gewicht')
    // The input rounds to one decimal as she types, so a second one never reaches the wire:
    // 4,25 is stored as 4,3. This is the only place that behaviour is pinned against the real
    // form rather than asserted about the component in isolation.
    await weight.fill('4,25')
    await weight.blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(weight).toHaveValue('4,3')
    // The unit sits beside the value, not in the label, and stays out of the value.
    await expect(weight).toHaveAccessibleDescription('kg')

    await page.reload()
    await expect(page.getByLabel('Gewicht')).toHaveValue('4,3')

    // Clearing it must not make the patient incomplete — the weight is optional, and an
    // "Unvollständig" badge here would mean it had crept into the completeness set.
    await page.getByLabel('Gewicht').fill('')
    await page.getByLabel('Gewicht').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Unvollständig')).toHaveCount(0)
  })

  test('a date of death archives the patient and hides it from the list', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId, 'Minka')
    await signIn(page)
    await page.goto(`/patients/${patientId}`)

    await page.getByLabel('Todesdatum').fill('04.05.2026')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Archiviert').first()).toBeVisible()

    await navigate(page, 'Patienten')
    await page.getByPlaceholder('Suchen …').fill('Minka')
    await expect(page.getByText('Keine Einträge')).toBeVisible()

    // Archived records are one toggle away, never gone.
    await page.getByLabel('Archivierte anzeigen').check()
    await expect(page.getByText('Minka').filter({ visible: true }).first()).toBeVisible()
  })
})
