import { describe, expect, it } from 'vitest'
import { combineSaveStatus } from '@/lib/saveState'

describe('combineSaveStatus', () => {
  it('shows a failure even when the other scope saved', () => {
    expect(
      combineSaveStatus({ state: 'saved', error: null }, { state: 'error', error: 'Netzwerk' }),
    ).toEqual({ state: 'error', error: 'Netzwerk' })
  })

  it('says "saving" while anything is still in flight', () => {
    expect(
      combineSaveStatus({ state: 'saved', error: null }, { state: 'saving', error: null }).state,
    ).toBe('saving')
  })

  it('says "saved" once one scope saved and the other has nothing to do', () => {
    expect(
      combineSaveStatus({ state: 'idle', error: null }, { state: 'saved', error: null }).state,
    ).toBe('saved')
  })

  it('stays idle with nothing to report', () => {
    expect(combineSaveStatus().state).toBe('idle')
  })
})
