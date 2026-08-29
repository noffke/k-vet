import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, apiFetch, setUnauthorizedHandler } from '@/api/fetcher'

/** A response shaped like the API's RFC 7807 problem documents. */
function problem(status: number, detail = 'nope'): Response {
  return new Response(JSON.stringify({ detail }), {
    status,
    headers: { 'content-type': 'application/problem+json' },
  })
}

function respondWith(response: Response): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.resolve(response)),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
  setUnauthorizedHandler(() => {})
})

describe('an expired session', () => {
  it('is reported once the server refuses a protected request', async () => {
    const expired = vi.fn()
    setUnauthorizedHandler(expired)
    respondWith(problem(401))

    // The caller still sees the failure — the redirect is an addition, not a replacement.
    await expect(apiFetch('/api/customers')).rejects.toBeInstanceOf(ApiError)
    expect(expired).toHaveBeenCalledTimes(1)
  })

  it('is not reported for a wrong password', async () => {
    const expired = vi.fn()
    setUnauthorizedHandler(expired)
    respondWith(problem(401))

    // `/api/auth/*` answers 401 for its own reasons; none of them end a session.
    await expect(apiFetch('/api/auth/login', { method: 'POST' })).rejects.toBeInstanceOf(ApiError)
    expect(expired).not.toHaveBeenCalled()
  })

  it('is not confused with a rejected write or a server fault', async () => {
    const expired = vi.fn()
    setUnauthorizedHandler(expired)

    respondWith(problem(422))
    await expect(apiFetch('/api/customers/1')).rejects.toBeInstanceOf(ApiError)
    respondWith(problem(500))
    await expect(apiFetch('/api/customers/1')).rejects.toBeInstanceOf(ApiError)

    expect(expired).not.toHaveBeenCalled()
  })
})
