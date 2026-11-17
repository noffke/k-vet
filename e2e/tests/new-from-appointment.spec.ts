import { expect, test } from '@playwright/test'
import { login } from '../fixtures/seed'
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

/**
 * An animal the practice has not seen yet is made on the visit it came to: named, its Tierart
 * and Geschlecht chosen, and on the visit at once — also for a customer just made, whose
 * address is still to come, and who has no animal at all yet.
 */
test.describe('a new animal from the appointment', () => {
  test('is made complete, goes onto the visit, and is the customer\'s', async ({
    page,
    request,
  }) => {
    const lastName = `Ohneadresse-${stamp()}`
    const animal = `Neuling-${stamp()}`
    await login(request)
    const customer = (await (await request.post('/api/customers')).json()) as { id: number }
    await request.patch(`/api/customers/${customer.id}`, {
      data: { salutation: 'herr', first_name: 'Max', last_name: lastName },
    })
    const appointment = (await (
      await request.post('/api/appointments', {
        data: { starts_at: new Date().toISOString(), customer_id: customer.id },
      })
    ).json()) as { id: number }

    await signIn(page)
    await page.goto(`/appointments/${appointment.id}`)
    // No animal yet, none to pick: the search offers only to make one.
    const picker = page.getByPlaceholder('Patienten')
    await picker.fill(animal)
    await page.getByRole('option', { name: 'Neues Tier …' }).click()

    const dialog = page.getByRole('dialog')
    await expect(dialog.getByLabel('Name')).toHaveValue(animal)
    const add = dialog.getByRole('button', { name: 'Hinzufügen' })
    await expect(add).toBeDisabled()
    await dialog.getByLabel('Tierart').fill('Hund')
    await dialog.getByLabel('Geschlecht').selectOption({ label: 'männlich' })
    await add.click()

    // On the visit, its tab open; the customer's only animal, so it stays there.
    await expect(dialog).toHaveCount(0)
    await expect(page.getByRole('tab', { name: new RegExp(animal) })).toHaveAttribute(
      'aria-selected',
      'true',
    )
    await expect(page.getByRole('tabpanel').getByRole('button', { name: 'Entfernen' })).toHaveCount(0)

    await page.goto(`/customers/${customer.id}`)
    await expect(page.getByRole('button', { name: new RegExp(`^${animal}`) }).filter({ visible: true })).toBeVisible()
  })
})
