import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError } from '@/api/fetcher'

/**
 * Auto-save (research R4, Constitution III — the top UX requirement).
 *
 * Every screen creates its record first and then PATCHes changed fields:
 *   - changes are merged into one pending patch and sent 600 ms after the last keystroke
 *   - `flush()` sends immediately (field blur, navigation, tab hidden)
 *   - the hook is field-level, so two fields edited in one burst travel in one request
 *   - 422 responses are surfaced per field; the stored record keeps its last valid value
 *
 * There is no save button anywhere in the application.
 */

export type SaveState = 'idle' | 'saving' | 'saved' | 'error'

export const AUTOSAVE_DEBOUNCE_MS = 600

export interface AutoSaveOptions<T extends object> {
  /** Sends the patch to the server and resolves with the updated record. */
  save: (patch: Partial<T>) => Promise<T>
  /** Called after a successful save — the place to update the query cache. */
  onSaved?: (record: T) => void
  debounceMs?: number
}

export interface AutoSave<T extends object> {
  /** Queues a field change. */
  set: (patch: Partial<T>) => void
  /** Sends queued changes right away. */
  flush: () => Promise<void>
  state: SaveState
  /** Field-level errors from the last rejected save. */
  fieldErrors: Record<string, string>
  /** Message of the last failed save, if it was not a field-level problem. */
  error: string | null
}

export function useAutoSave<T extends object>({
  save,
  onSaved,
  debounceMs = AUTOSAVE_DEBOUNCE_MS,
}: AutoSaveOptions<T>): AutoSave<T> {
  const [state, setState] = useState<SaveState>('idle')
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)

  const pending = useRef<Partial<T>>({})
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const inFlight = useRef<Promise<void> | null>(null)
  // Keeping the callbacks in refs lets callers pass inline closures without
  // restarting the debounce on every render.
  const saveRef = useRef(save)
  const onSavedRef = useRef(onSaved)
  saveRef.current = save
  onSavedRef.current = onSaved

  const send = useCallback(async () => {
    if (timer.current) {
      clearTimeout(timer.current)
      timer.current = null
    }
    const patch = pending.current
    if (Object.keys(patch).length === 0) return
    pending.current = {}

    setState('saving')
    const request = (async () => {
      try {
        const record = await saveRef.current(patch)
        setFieldErrors({})
        setError(null)
        setState('saved')
        onSavedRef.current?.(record)
      } catch (caught) {
        if (caught instanceof ApiError && caught.fieldErrors.length > 0) {
          setFieldErrors(
            Object.fromEntries(caught.fieldErrors.map((field) => [field.field, field.message])),
          )
          setError(null)
        } else {
          setError(caught instanceof Error ? caught.message : String(caught))
        }
        setState('error')
      }
    })()
    inFlight.current = request
    await request
  }, [])

  const flush = useCallback(async () => {
    await send()
    // A change queued while a request was running must not be lost.
    if (Object.keys(pending.current).length > 0) await send()
  }, [send])

  const set = useCallback(
    (patch: Partial<T>) => {
      pending.current = { ...pending.current, ...patch }
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => {
        void send()
      }, debounceMs)
    },
    [send, debounceMs],
  )

  // Leaving the page must not drop the last keystrokes.
  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === 'hidden') void flush()
    }
    const onPageHide = () => {
      void flush()
    }
    document.addEventListener('visibilitychange', onHide)
    window.addEventListener('pagehide', onPageHide)
    return () => {
      document.removeEventListener('visibilitychange', onHide)
      window.removeEventListener('pagehide', onPageHide)
      // Unmount (e.g. route change) flushes as well.
      void flush()
    }
  }, [flush])

  return { set, flush, state, fieldErrors, error }
}
