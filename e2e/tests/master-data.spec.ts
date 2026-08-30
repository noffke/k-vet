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

  test('a stored address and its type are edited in place', async ({ page, request }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page)
    await page.goto(`/customers/${customerId}`)

    // The row is a form like any other on this page, not a line of text with a bin next to it.
    const address = page.getByLabel('E-Mail').first()
    await expect(address).toHaveValue('erika@example.com')
    await address.fill('erika.neu@example.com')
    await address.blur()

    // Saved on blur, like every other field — no button to press.
    await page.reload()
    await expect(page.getByLabel('E-Mail').first()).toHaveValue('erika.neu@example.com')

    // The type is stored the moment it is chosen.
    await page.getByLabel('Art').first().selectOption('work')
    await page.reload()
    await expect(page.getByLabel('Art').first()).toHaveValue('work')

    // A rejected address reports on its own row and keeps what was stored.
    const broken = page.getByLabel('E-Mail').first()
    await broken.fill('kaputt(at)example.com')
    await broken.blur()
    await expect(page.getByText('Keine gültige E-Mail-Adresse')).toBeVisible()
    await page.reload()
    await expect(page.getByLabel('E-Mail').first()).toHaveValue('erika.neu@example.com')
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

  test('a detail page can be left the way it was entered', async ({ page, request }) => {
    const { customerId, lastName } = await seedCustomer(request)
    const patientId = await seedPatient(request, customerId, 'Rufus')
    await signIn(page)
    await page.goto(`/patients/${patientId}`)

    // An animal is reached through its customer, so that is where "back" goes — there is no
    // patient list any more (issues.md 9).
    const back = page.getByRole('link', { name: new RegExp(`Zurück: .*${lastName}`) })
    await expect(back).toBeVisible()

    // The header buttons shrink to their icon on a phone. Their names have to survive it,
    // or the phone viewport — a first-class target — gets unlabelled buttons.
    await expect(page.getByRole('button', { name: 'Archivieren', exact: true })).toBeVisible()

    await back.click()
    await expect(page).toHaveURL(new RegExp(`/customers/${customerId}$`))
    await expect(page.getByRole('heading', { name: 'Patienten' })).toBeVisible()
  })

  test('the Rasse field suggests what the practice already sees', async ({ page, request }) => {
    const race = `Havaneser-${Date.now().toString().slice(-6)}`
    const { customerId } = await seedCustomer(request)
    const first = await seedPatient(request, customerId, 'Eddie')
    const second = await seedPatient(request, customerId, 'Bella')
    await signIn(page)

    // Record the breed once …
    await page.goto(`/patients/${first}`)
    await page.getByLabel('Tierart').fill('Hund')
    await page.getByLabel('Rasse').fill(race)
    await page.getByLabel('Rasse').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // … and the next dog is offered it.
    // The fixture already records "Hund", so nothing is saved here — the suggestions are
    // fetched for the species the record holds.
    await page.goto(`/patients/${second}`)
    await expect
      .poll(async () =>
        page
          .locator('#race-options option')
          .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('value'))),
      )
      .toContain(race)

    // A rabbit is not offered a dog's breeds.
    await page.getByLabel('Tierart').fill('Kaninchen')
    await page.getByLabel('Tierart').blur()
    await expect
      .poll(async () =>
        page
          .locator('#race-options option')
          .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('value'))),
      )
      .not.toContain(race)
  })

  test("a date of death archives the patient and takes it off the customer", async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    const name = `Minka-${Date.now().toString().slice(-6)}`
    const patientId = await seedPatient(request, customerId, name)
    await signIn(page)

    // The customer lists the animal to begin with, on the page and in the customers index.
    await page.goto(`/customers/${customerId}`)
    await expect(page.getByRole('button', { name: new RegExp(name) })).toBeVisible()

    await page.goto(`/patients/${patientId}`)
    await page.getByLabel('Todesdatum').fill('04.05.2026')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await expect(page.getByText('Archiviert').first()).toBeVisible()

    // Archived: gone from the owner's list of animals, and from the name the index shows.
    await page.goto(`/customers/${customerId}`)
    await expect(page.getByRole('button', { name: new RegExp(name) })).toHaveCount(0)

    await navigate(page, 'Kunden')
    await page.getByPlaceholder('Suchen …').fill(name)
    await expect(page.getByText('Keine Einträge')).toBeVisible()
  })
})
