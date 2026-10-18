import { afterEach, describe, expect, it, vi } from 'vitest'
import { entryAt, remember } from '@/lib/trail'

afterEach(() => {
  vi.restoreAllMocks()
  sessionStorage.clear()
})

describe('the trail of pages', () => {
  it('names the page at each history position', () => {
    remember(3, { title: 'Bello', href: '/patients/7' })
    remember(4, { title: '18.10.2026', href: '/appointments/9?tier=12' })
    expect(entryAt(3)).toEqual({ title: 'Bello', href: '/patients/7' })
    expect(entryAt(4)?.href).toBe('/appointments/9?tier=12')
  })

  it('lets a later page at the same position take its place', () => {
    remember(2, { title: 'Termine', href: '/appointments' })
    remember(2, { title: 'Kunden', href: '/customers' })
    expect(entryAt(2)?.title).toBe('Kunden')
  })

  it('knows nothing before the first page or about pages this tab never saw', () => {
    expect(entryAt(-1)).toBeUndefined()
    expect(entryAt(5)).toBeUndefined()
  })

  it('falls back to nothing when the browser keeps no storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(() => remember(1, { title: 'Kunden', href: '/customers' })).not.toThrow()
    expect(entryAt(1)).toBeUndefined()
  })
})
