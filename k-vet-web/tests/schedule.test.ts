import { describe, expect, it } from 'vitest'
import { defaultSchedule, isAhead, scheduleAt, scheduleOf } from '@/lib/schedule'

describe('sending later', () => {
  it('offers tomorrow at eight', () => {
    expect(defaultSchedule(new Date(2026, 9, 23, 16, 30))).toEqual({ date: '2026-10-24', hour: 8 })
  })

  it('finds tomorrow across the end of a month and of a year', () => {
    expect(defaultSchedule(new Date(2026, 9, 31, 22, 0)).date).toBe('2026-11-01')
    expect(defaultSchedule(new Date(2026, 11, 31, 9, 0)).date).toBe('2027-01-01')
  })

  it('sends the local hour that was chosen', () => {
    const instant = new Date(scheduleAt({ date: '2026-10-24', hour: 8 }))
    expect(instant.getFullYear()).toBe(2026)
    expect(instant.getMonth()).toBe(9)
    expect(instant.getDate()).toBe(24)
    expect(instant.getHours()).toBe(8)
    expect(instant.getMinutes()).toBe(0)
  })

  it('reopens a scheduled instant as the day and hour it was chosen as', () => {
    const chosen = { date: '2026-12-01', hour: 17 }
    expect(scheduleOf(scheduleAt(chosen))).toEqual(chosen)
  })

  it('tells a time still ahead from one already past', () => {
    const now = new Date(2026, 9, 23, 12, 0)
    expect(isAhead(new Date(2026, 9, 23, 13, 0).toISOString(), now)).toBe(true)
    expect(isAhead(new Date(2026, 9, 23, 11, 0).toISOString(), now)).toBe(false)
    expect(isAhead(null, now)).toBe(false)
  })
})
