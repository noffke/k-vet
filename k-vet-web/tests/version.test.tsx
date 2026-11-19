import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { apiFetch, writesSettled } from '@/api/fetcher'
import { UpdateBanner } from '@/components/UpdateBanner'
import i18n from '@/lib/i18n'
import {
  checkServerVersion,
  noteServerVersion,
  resetVersionForTests,
  useNewVersion,
  useVersionWatch,
  VERSION_CHECK_INTERVAL_MS,
} from '@/lib/version'

/** An answer from a server running `version`, or one that does not say. */
function answer(version: string | null, status = 200): Response {
  return new Response(status === 204 ? null : '{}', {
    status,
    headers: {
      'content-type': 'application/json',
      ...(version === null ? {} : { 'x-kvet-version': version }),
    },
  })
}

function Probe() {
  return <p data-testid="new">{useNewVersion() ?? 'none'}</p>
}

function Watcher() {
  useVersionWatch()
  return null
}

beforeAll(async () => {
  await i18n.changeLanguage('de')
})

beforeEach(() => {
  resetVersionForTests()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('the release this page is', () => {
  it('is the first one heard from; only a different one is news', () => {
    render(<Probe />)
    act(() => noteServerVersion('1.4.0'))
    expect(screen.getByTestId('new')).toHaveTextContent('none')

    act(() => noteServerVersion('1.4.0'))
    expect(screen.getByTestId('new')).toHaveTextContent('none')

    act(() => noteServerVersion('1.4.1'))
    expect(screen.getByTestId('new')).toHaveTextContent('1.4.1')
  })

  it('is not taken from an answer that does not say', () => {
    render(<Probe />)
    act(() => {
      noteServerVersion(null)
      noteServerVersion('  ')
      noteServerVersion('1.4.0')
      noteServerVersion(null)
    })
    expect(screen.getByTestId('new')).toHaveTextContent('none')
    act(() => noteServerVersion('1.5.0'))
    expect(screen.getByTestId('new')).toHaveTextContent('1.5.0')
  })

  it('is heard from every API answer, a refusal too', async () => {
    render(<Probe />)
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(answer('1.4.0'))),
    )
    await apiFetch('/api/customers')
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(answer('1.4.1', 422))),
    )
    await expect(apiFetch('/api/customers/1', { method: 'PATCH', body: '{}' })).rejects.toThrow()
    expect(screen.getByTestId('new')).toHaveTextContent('1.4.1')
  })

  it('is asked of the health probe when the tab is shown again, and every few minutes', async () => {
    vi.useFakeTimers()
    const probe = vi.fn((_url: string, _init?: RequestInit) => Promise.resolve(answer('1.4.0')))
    vi.stubGlobal('fetch', probe)
    render(<Watcher />)

    document.dispatchEvent(new Event('visibilitychange'))
    expect(probe).toHaveBeenCalledTimes(1)
    expect(probe.mock.calls[0]?.[0]).toBe('/healthz')
    expect(probe.mock.calls[0]?.[1]).toMatchObject({ method: 'HEAD', cache: 'no-store' })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(VERSION_CHECK_INTERVAL_MS)
    })
    expect(probe).toHaveBeenCalledTimes(2)
  })

  it('stays unknown while the server does not answer', async () => {
    render(<Probe />)
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
    )
    await checkServerVersion()
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(answer('1.4.0'))),
    )
    await checkServerVersion()
    act(() => noteServerVersion('1.4.1'))
    expect(screen.getByTestId('new')).toHaveTextContent('1.4.1')
  })
})

describe('waiting for the writes', () => {
  it('waits for a write under way, and for a quiet moment after it', async () => {
    let answerWrite: (response: Response) => void = () => undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            answerWrite = resolve
          }),
      ),
    )
    const write = apiFetch('/api/customers/1', { method: 'PATCH', body: '{}' })
    let settled = false
    const waiting = writesSettled({ quietMs: 100 }).then(() => {
      settled = true
    })

    await new Promise((resolve) => setTimeout(resolve, 200))
    expect(settled).toBe(false)

    answerWrite(answer('1.4.0'))
    await write
    await waiting
    expect(settled).toBe(true)
  })

  it('does not count reads', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise<Response>(() => undefined)),
    )
    void apiFetch('/api/customers')
    const started = Date.now()
    await writesSettled({ quietMs: 50 })
    expect(Date.now() - started).toBeLessThan(500)
  })
})

describe('the update notice', () => {
  it('shows only once a newer release is running', () => {
    render(<UpdateBanner />)
    expect(screen.queryByRole('status')).toBeNull()
    act(() => {
      noteServerVersion('1.4.0')
      noteServerVersion('1.4.1')
    })
    expect(screen.getByRole('status')).toHaveTextContent('k-vet 1.4.1 ist installiert.')
  })

  it('reloads only after the field the vet is in has saved', async () => {
    const reload = vi.fn()
    vi.stubGlobal('location', { ...window.location, reload })
    let answerWrite: (response: Response) => void = () => undefined
    const server = vi.fn(
      (_url: string, _init?: RequestInit) =>
        new Promise<Response>((resolve) => {
          answerWrite = resolve
        }),
    )
    vi.stubGlobal('fetch', server)
    render(
      <>
        <input
          aria-label="Bemerkung"
          onBlur={() => void apiFetch('/api/customers/1', { method: 'PATCH', body: '{}' })}
        />
        <UpdateBanner />
      </>,
    )
    act(() => {
      noteServerVersion('1.4.0')
      noteServerVersion('1.4.1')
    })
    screen.getByLabelText('Bemerkung').focus()

    fireEvent.click(screen.getByRole('button', { name: 'Neu laden' }))
    // Leaving the field sent its save; the page waits for it, past any quiet moment.
    await screen.findByRole('button', { name: 'Wird gespeichert …' })
    expect(server).toHaveBeenCalledTimes(1)
    expect(server.mock.calls[0]?.[1]).toMatchObject({ method: 'PATCH' })
    await new Promise((resolve) => setTimeout(resolve, 1200))
    expect(reload).not.toHaveBeenCalled()

    answerWrite(answer('1.4.1'))
    await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1), { timeout: 3000 })
  })
})
