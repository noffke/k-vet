import { expect, test } from '@playwright/test'
import { navigate, signIn } from './helpers'

/**
 * A session that runs out while the tab sits open.
 *
 * Sessions end on inactivity, so the vet finds out by clicking — and until the app noticed
 * the 401 she could keep clicking through screens that rendered nothing. Now the first
 * refused request says so and returns her to the sign-in screen.
 */
test.describe('an expired session', () => {
  test('says so and returns to the login screen instead of emptying the page', async ({ page }) => {
    await signIn(page)
    await navigate(page, 'Kunden')
    await expect(page).toHaveURL(/\/customers$/)

    // Dropping the cookie is what an expired session looks like from the browser's side.
    await page.context().clearCookies()

    await navigate(page, 'Apotheke')

    // The reason is in the URL, so it is still on screen once the toast has faded.
    await expect(page).toHaveURL(/\/login\?expired=true$/)
    await expect(
      page.getByRole('status').filter({ hasText: 'Ihre Sitzung ist abgelaufen' }).first(),
    ).toBeVisible()

    // And signing in again lands on a working app, not on the stale screen.
    await signIn(page)
    await navigate(page, 'Kunden')
    await expect(page.getByPlaceholder('Suchen …').first()).toBeVisible()
  })
})
