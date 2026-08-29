import { useNavigate, useSearch } from '@tanstack/react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'
import { useLogin } from '@/features/auth/session'

/**
 * Sign-in screen. The practice wordmark sets the tone: condensed caps on cream, the
 * same voice as the practice's own site.
 */
export function LoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { expired } = useSearch({ from: '/login' })
  const signIn = useLogin()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    try {
      await signIn.mutateAsync({ username, password })
      await navigate({ to: '/' })
    } catch {
      // The message is rendered from the mutation's error state below.
    }
  }

  const message =
    signIn.error instanceof ApiError
      ? signIn.error.status === 429
        ? t('login.throttled')
        : t('login.failed')
      : signIn.error
        ? t('error.generic')
        : null

  return (
    <main className="flex min-h-dvh items-center justify-center bg-cream px-4 py-10">
      <div className="w-full max-w-sm">
        <p className="text-[0.6875rem] font-bold uppercase tracking-[0.18em] text-rust">
          {t('app.practice')}
        </p>
        <h1 className="mt-1 text-3xl font-bold uppercase tracking-tight text-rust">k-vet</h1>
        <p className="mt-1 text-sm text-ink-soft">{t('login.subtitle')}</p>

        <form
          onSubmit={submit}
          className="mt-6 flex flex-col gap-4 rounded-card border border-line bg-surface p-5"
        >
          <TextField
            label={t('login.username')}
            value={username}
            autoComplete="username"
            autoCapitalize="none"
            // The single user lands here on every fresh session; save them a tap.
            autoFocus
            onChange={(event) => setUsername(event.target.value)}
          />
          <TextField
            label={t('login.password')}
            type="password"
            value={password}
            autoComplete="current-password"
            onChange={(event) => setPassword(event.target.value)}
          />
          {/* A failed attempt is the more urgent thing to read, so it replaces the notice
              that the previous session had run out. */}
          {message ? (
            <p className="text-sm font-medium text-danger" role="alert">
              {message}
            </p>
          ) : expired ? (
            <p className="text-sm text-ink-soft" role="status">
              {t('login.expired')}
            </p>
          ) : null}
          <Button type="submit" variant="primary" disabled={signIn.isPending}>
            {t('login.submit')}
          </Button>
        </form>
      </div>
    </main>
  )
}
