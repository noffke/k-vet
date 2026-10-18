import type { SaveState } from '@/lib/autosave'

/** One auto-save scope's state as the indicator shows it. */
export interface SaveStatus {
  state: SaveState
  error: string | null
}

const PRECEDENCE: Record<SaveState, number> = { error: 3, saving: 2, saved: 1, idle: 0 }

/**
 * One indicator for a page with several auto-save scopes — the appointment and the animal's
 * record share a page, and two "Gespeichert" side by side would crowd the phone's header and
 * say nothing the one does not.
 *
 * A failure anywhere wins, so it is never hidden behind a success elsewhere; then anything
 * still in flight; "saved" only once nothing is.
 */
export function combineSaveStatus(...statuses: SaveStatus[]): SaveStatus {
  return statuses.reduce<SaveStatus>(
    (worst, status) => (PRECEDENCE[status.state] > PRECEDENCE[worst.state] ? status : worst),
    { state: 'idle', error: null },
  )
}
