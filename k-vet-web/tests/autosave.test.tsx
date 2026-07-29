import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/api/fetcher'
import { AUTOSAVE_DEBOUNCE_MS, useAutoSave } from '@/lib/autosave'

interface Customer extends Record<string, unknown> {
  id: number
  first_name?: string | null
  last_name?: string | null
}

function Harness({
  save,
  onSaved,
}: {
  save: (patch: Partial<Customer>) => Promise<Customer>
  onSaved?: (record: Customer) => void
}) {
  const autoSave = useAutoSave<Customer>({ save, onSaved })
  return (
    <div>
      <span data-testid="state">{autoSave.state}</span>
      <span data-testid="errors">{JSON.stringify(autoSave.fieldErrors)}</span>
      <button type="button" onClick={() => autoSave.set({ first_name: 'Erika' })}>
        type first name
      </button>
      <button type="button" onClick={() => autoSave.set({ last_name: 'Mustermann' })}>
        type last name
      </button>
      <button type="button" onClick={() => void autoSave.flush()}>
        blur
      </button>
    </div>
  )
}

describe('useAutoSave', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('debounces and merges changes into a single request', async () => {
    const save = vi.fn(async (patch: Partial<Customer>) => ({ id: 1, ...patch }) as Customer)
    render(<Harness save={save} />)

    screen.getByText('type first name').click()
    screen.getByText('type last name').click()
    expect(save).not.toHaveBeenCalled()

    await act(async () => {
      vi.advanceTimersByTime(AUTOSAVE_DEBOUNCE_MS)
    })

    expect(save).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenCalledWith({ first_name: 'Erika', last_name: 'Mustermann' })
    expect(screen.getByTestId('state')).toHaveTextContent('saved')
  })

  it('flushes immediately on blur, without waiting for the debounce', async () => {
    const save = vi.fn(async (patch: Partial<Customer>) => ({ id: 1, ...patch }) as Customer)
    render(<Harness save={save} />)

    screen.getByText('type first name').click()
    await act(async () => {
      screen.getByText('blur').click()
    })

    expect(save).toHaveBeenCalledTimes(1)
  })

  it('reports field errors and keeps them until the next successful save', async () => {
    const save = vi
      .fn<(patch: Partial<Customer>) => Promise<Customer>>()
      .mockRejectedValueOnce(
        new ApiError(422, 'validation failed', [{ field: 'last_name', message: 'field.required' }]),
      )
      .mockResolvedValueOnce({ id: 1, last_name: 'Mustermann' })
    render(<Harness save={save} />)

    screen.getByText('type last name').click()
    await act(async () => {
      vi.advanceTimersByTime(AUTOSAVE_DEBOUNCE_MS)
    })

    expect(screen.getByTestId('state')).toHaveTextContent('error')
    expect(screen.getByTestId('errors')).toHaveTextContent('field.required')

    screen.getByText('type last name').click()
    await act(async () => {
      vi.advanceTimersByTime(AUTOSAVE_DEBOUNCE_MS)
    })

    expect(screen.getByTestId('state')).toHaveTextContent('saved')
    expect(screen.getByTestId('errors')).toHaveTextContent('{}')
  })

  it('notifies the caller so the query cache can be updated', async () => {
    const onSaved = vi.fn()
    const save = vi.fn(async () => ({ id: 7, first_name: 'Erika' }) as Customer)
    render(<Harness save={save} onSaved={onSaved} />)

    screen.getByText('type first name').click()
    await act(async () => {
      vi.advanceTimersByTime(AUTOSAVE_DEBOUNCE_MS)
    })

    expect(onSaved).toHaveBeenCalledWith({ id: 7, first_name: 'Erika' })
  })

  it('flushes pending changes when the tab is hidden', async () => {
    const save = vi.fn(async (patch: Partial<Customer>) => ({ id: 1, ...patch }) as Customer)
    render(<Harness save={save} />)

    screen.getByText('type first name').click()
    await act(async () => {
      vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden')
      document.dispatchEvent(new Event('visibilitychange'))
    })

    expect(save).toHaveBeenCalledTimes(1)
  })
})
