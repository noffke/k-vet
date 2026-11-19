import { useEffect, useSyncExternalStore } from 'react'

/**
 * Which k-vet release this page is, and whether a newer one is running.
 *
 * Every response names the release that gave it (`x-kvet-version`). The page has no version of
 * its own compiled in — the image builds it without one, so the build the tests run is the one
 * shipped — so it takes the first release it hears from: the server that served it a moment
 * earlier. A different one later means a new release has been deployed, and the page offers to
 * reload (`UpdateBanner`).
 */
export const VERSION_HEADER = 'x-kvet-version'

/** How often a visible page asks, for the laptop left on one screen. */
export const VERSION_CHECK_INTERVAL_MS = 5 * 60 * 1000

let loaded: string | null = null
let available: string | null = null
const listeners = new Set<() => void>()

/** Takes note of the release a response came from; a missing or empty one says nothing. */
export function noteServerVersion(value: string | null | undefined): void {
  const version = value?.trim()
  if (!version) return
  if (loaded === null) {
    loaded = version
    return
  }
  const next = version === loaded ? null : version
  if (next === available) return
  available = next
  for (const listener of listeners) listener()
}

/** The newer release that is running, or `null` while it is still this page's own. */
export function useNewVersion(): string | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    () => available,
  )
}

/**
 * Asks the health probe which release answers — open, cheap, and outside the session. A server
 * that is restarting does not answer; the next check will.
 */
export async function checkServerVersion(): Promise<void> {
  try {
    const response = await fetch('/healthz', { method: 'HEAD', cache: 'no-store' })
    noteServerVersion(response.headers.get(VERSION_HEADER))
  } catch {
    // Unreachable for now: nothing to learn.
  }
}

/**
 * The app makes no requests while the vet is not using it, so it asks by itself: when the tab
 * is shown again — the phone taken out of the pocket — and every few minutes while it is shown.
 */
export function useVersionWatch(): void {
  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void checkServerVersion()
    }
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void checkServerVersion()
    }, VERSION_CHECK_INTERVAL_MS)
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [])
}

/** Forgets what was heard, for tests that start a page afresh. */
export function resetVersionForTests(): void {
  loaded = null
  available = null
  for (const listener of listeners) listener()
}
