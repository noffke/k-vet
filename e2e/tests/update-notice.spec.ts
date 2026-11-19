import { expect, type Page, test } from '../fixtures/test'
import { seedCustomer } from '../fixtures/seed'
import { signIn } from './helpers'

/**
 * A new release deployed while a tab is open: every answer names the release that gave it
 * (`x-kvet-version`), so the page notices one that is not its own and offers to reload — and
 * reloading keeps what the vet was typing. The new release is played by rewriting that header
 * on the real backend's answers.
 */
const NEXT = 'next-release'

/** From now on, `pattern`'s answers come from the next release. */
async function nextReleaseAnswers(page: Page, pattern: string): Promise<void> {
  await page.route(pattern, async (route) => {
    const response = await route.fetch()
    await route.fulfill({ response, headers: { ...response.headers(), 'x-kvet-version': NEXT } })
  })
}

const notice = (page: Page) => page.getByRole('status').filter({ hasText: 'ist installiert' })

test.describe('a new release', () => {
  test('is noticed on the next answer, and reloading keeps what was typed', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page, `/customers/${customerId}`)
    const phone = page.getByLabel('Telefon', { exact: true })
    await expect(phone).toHaveValue('030 12345678')
    await expect(notice(page)).toHaveCount(0)

    // Deployed now: the next answer — this change's auto-save — comes from it.
    await nextReleaseAnswers(page, '**/api/**')
    await phone.fill('030 1111')
    await expect(notice(page)).toContainText(`k-vet ${NEXT} ist installiert.`)

    // Typed, and the reload pressed at once: the change goes out before the page does.
    await phone.fill('030 2222')
    const reloaded = page.waitForEvent('load')
    await notice(page).getByRole('button', { name: 'Neu laden' }).click()
    await reloaded

    // The reloaded page is the new release's own: nothing to announce.
    await expect(phone).toHaveValue('030 2222')
    await expect(notice(page)).toHaveCount(0)
    const stored = await (await request.get(`/api/customers/${customerId}`)).json()
    // Stored in international form; the page shows it as typed.
    expect(stored.phone).toBe('+49302222')
  })

  test('is asked about when the tab is shown again, without any other request', async ({
    page,
    request,
  }) => {
    const { customerId } = await seedCustomer(request)
    await signIn(page, `/customers/${customerId}`)
    // Loaded, so the page has heard which release it is.
    await expect(page.getByLabel('Telefon', { exact: true })).toHaveValue('030 12345678')

    await nextReleaseAnswers(page, '**/healthz')
    await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')))
    await expect(notice(page)).toContainText(`k-vet ${NEXT} ist installiert.`)
  })
})
