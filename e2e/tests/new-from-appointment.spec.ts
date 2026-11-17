import { expect, test } from '@playwright/test'
import { navigate, signIn } from './helpers'

const stamp = () => Date.now().toString().slice(-6)

/**
 * A caller who is not a customer yet is made from the appointment itself: named in a small
 * dialog, set on the appointment, and opened for the rest. "Zurück" then retraces the way —
 * from an animal made on that page, to the customer, to the appointment — and the appointment
 * shows them chosen.
 */
test.describe('a new customer from the appointment', () => {
  test('is made, chosen, and found again on the way back from their new animal', async ({
    page,
  }) => {
    const lastName = `Neukundin-${stamp()}`
    const animal = `Neutier-${stamp()}`
    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByRole('button', { name: 'Neuer Termin' }).click()
    await expect(page).toHaveURL(/\/appointments\/\d+$/)
    const appointmentUrl = page.url()
    await page.getByLabel('Uhrzeit').fill('11:45')
    await page.getByLabel('Uhrzeit').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Nobody of that name yet: the search says so, and offers to make them.
    await page.getByPlaceholder('Kunde suchen …').fill(`Erika ${lastName}`)
    const newCustomer = page.getByRole('option', { name: 'Neuer Kunde …' })
    const customers = page.getByRole('listbox').filter({ has: newCustomer })
    await expect(customers.getByText('Keine Einträge')).toBeVisible()
    await newCustomer.click()

    // What was typed is the name; the Anrede is asked, and nothing is made without it.
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByLabel('Vorname')).toHaveValue('Erika')
    await expect(dialog.getByLabel('Nachname')).toHaveValue(lastName)
    const create = dialog.getByRole('button', { name: 'Anlegen und öffnen' })
    await expect(create).toBeDisabled()
    await dialog.getByLabel('Anrede').selectOption({ label: 'Frau' })
    await create.click()

    // Their page opens for the address and the animals.
    await expect(page).toHaveURL(/\/customers\/\d+$/)
    await expect(page.getByRole('heading', { name: new RegExp(lastName) })).toBeVisible()
    // An animal waits for the address, as on any new customer.
    const newAnimal = page.getByRole('button', { name: 'Neuer Patient' })
    await expect(newAnimal).toBeDisabled()
    await page.getByLabel('Straße und Hausnummer').fill('Musterweg 5')
    await page.getByLabel('PLZ').fill('12345')
    await page.getByLabel('Ort').fill('Musterstadt')
    await expect(page.getByRole('status')).toHaveText('Gespeichert')
    await newAnimal.click()
    await expect(page).toHaveURL(/\/patients\/\d+$/)
    await page.getByLabel('Name', { exact: true }).fill(animal)
    await page.getByLabel('Tierart').fill('Hund')
    await page.getByLabel('Geschlecht').selectOption({ label: 'männlich' })
    await page.getByLabel('Tierart').blur()
    await expect(page.getByRole('status')).toHaveText('Gespeichert')

    // Back the way it came: the animal leads to the customer, the customer to the appointment.
    await page.getByRole('link', { name: new RegExp(`^Zurück: .*${lastName}`) }).click()
    await expect(page).toHaveURL(/\/customers\/\d+$/)
    await page.getByRole('link', { name: /^Zurück: \d{2}\.\d{2}\.\d{4}/ }).click()
    await expect(page).toHaveURL(new RegExp(`^${appointmentUrl}`))

    // The appointment is theirs, and the animal made since is offered for the visit.
    await expect(page.getByPlaceholder('Kunde suchen …')).toHaveCount(0)
    await expect(page.getByText(`Erika ${lastName}`).filter({ visible: true }).first()).toBeVisible()
    await page.getByRole('option', { name: new RegExp(`^${animal}`) }).click()
    await expect(page.getByRole('tab', { name: new RegExp(animal) })).toBeVisible()
  })

  test('cancelling the dialog makes nobody', async ({ page }) => {
    const lastName = `Niemand-${stamp()}`
    await signIn(page)
    await navigate(page, 'Termine')
    await page.getByRole('button', { name: 'Neuer Termin' }).click()
    await expect(page).toHaveURL(/\/appointments\/\d+$/)

    await page.getByPlaceholder('Kunde suchen …').fill(lastName)
    await page.getByRole('option', { name: 'Neuer Kunde …' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Abbrechen' }).click()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(page).toHaveURL(/\/appointments\/\d+$/)

    // Not even an incomplete one, which the picker would not show: asked of the server.
    const found = await page.request.get(`/api/customers?q=${encodeURIComponent(lastName)}`)
    expect(await found.json()).toEqual([])
    await expect(page.getByPlaceholder('Kunde suchen …')).toBeVisible()
  })
})
