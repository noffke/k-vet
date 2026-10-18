import { useNavigate, useRouter, useRouterState } from '@tanstack/react-router'

/**
 * Where "Zurück" leads: the page the vet came from, not a fixed parent (issues.md — customer →
 * animal → appointment went back to all appointments).
 *
 * The browser keeps the history; what it does not keep is what each entry *was*. Every page
 * title is remembered against the position of its history entry (`__TSR_index`, which TanStack
 * puts on every entry), so the entry before the current one can be named and returned to. It
 * lives in `sessionStorage`: per tab, like the history itself, and surviving a reload the way
 * the history does.
 */
const STORAGE_KEY = 'kvet.trail'

export interface TrailEntry {
  title: string
  href: string
}

function read(): Record<string, TrailEntry> {
  try {
    const stored = sessionStorage.getItem(STORAGE_KEY)
    return stored ? (JSON.parse(stored) as Record<string, TrailEntry>) : {}
  } catch {
    // No storage (a locked-down browser): every page falls back to its fixed parent.
    return {}
  }
}

/** Records what the history entry at `index` is. A later page at the same index overwrites it. */
export function remember(index: number, entry: TrailEntry): void {
  try {
    const trail = read()
    trail[String(index)] = entry
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(trail))
  } catch {
    // Without storage there is no trail, and nothing to record it in.
  }
}

/** What the history entry at `index` was, when this tab has seen it. */
export function entryAt(index: number): TrailEntry | undefined {
  return index < 0 ? undefined : read()[String(index)]
}

/** The current history entry's position and address. */
export function useHistoryEntry(): { index: number; href: string } {
  const index = useRouterState({ select: (state) => state.location.state.__TSR_index ?? 0 })
  const href = useRouterState({ select: (state) => state.location.href })
  return { index, href }
}

/** The page before this one, when this tab came from a page of the app. */
export function usePreviousPage(): TrailEntry | undefined {
  const { index } = useHistoryEntry()
  return entryAt(index - 1)
}

/**
 * Leaves a page the way "Zurück" would: back through the history when the vet came from a page
 * of the app — the appointment returns with its animal's tab, the list with its scroll — and to
 * `fallback` otherwise (a link opened in a new tab), replacing this entry so a later "Zurück"
 * does not lead here again.
 */
export function useGoBack(fallback: { to: string; params?: Record<string, string> }) {
  const router = useRouter()
  const navigate = useNavigate()
  const previous = usePreviousPage()
  return async () => {
    if (previous) {
      router.history.back()
    } else {
      await navigate({ to: fallback.to, params: fallback.params, replace: true })
    }
  }
}
