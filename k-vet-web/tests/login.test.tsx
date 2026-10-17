import { render, screen } from '@testing-library/react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { LoginPage } from '@/features/auth/LoginPage'
import i18n from '@/lib/i18n'

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => vi.fn(),
  useSearch: () => ({ expired: false }),
}))
vi.mock('@/features/auth/session', () => ({
  useLogin: () => ({ mutateAsync: vi.fn(), error: null, isPending: false }),
}))

/**
 * Password managers recognise a sign-in by its markup alone: a form, named fields, their
 * autocomplete roles, a submit button. Nothing here is visible, so nothing would notice it
 * going missing — this does.
 */
describe('LoginPage', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('de-DE')
  })

  it('carries what autofill looks for', () => {
    const { container } = render(<LoginPage />)

    const username = screen.getByLabelText('Benutzername')
    expect(username).toHaveAttribute('name', 'username')
    expect(username).toHaveAttribute('autocomplete', 'username')

    const password = screen.getByLabelText('Passwort')
    expect(password).toHaveAttribute('name', 'password')
    expect(password).toHaveAttribute('type', 'password')
    expect(password).toHaveAttribute('autocomplete', 'current-password')

    const form = container.querySelector('form')
    expect(form).toContainElement(username)
    expect(form).toContainElement(password)
    expect(screen.getByRole('button', { name: 'Anmelden' })).toHaveAttribute('type', 'submit')
  })
})
