import { describe, expect, it } from 'vitest'
import { mergePages, nextOffset, PAGE_SIZE } from '@/lib/paging'

const page = (from: number, count: number) =>
  Array.from({ length: count }, (_, index) => ({ id: from + index }))

describe('nextOffset', () => {
  it('asks for the next page while the last one came back full', () => {
    expect(nextOffset(page(1, PAGE_SIZE), [page(1, PAGE_SIZE)])).toBe(PAGE_SIZE)
    expect(nextOffset(page(51, PAGE_SIZE), [page(1, PAGE_SIZE), page(51, PAGE_SIZE)])).toBe(
      2 * PAGE_SIZE,
    )
  })

  it('stops once a page comes back short — the list is complete', () => {
    expect(nextOffset(page(1, 3), [page(1, 3)])).toBeUndefined()
    expect(nextOffset([], [page(1, PAGE_SIZE), []])).toBeUndefined()
  })
})

describe('mergePages', () => {
  it('keeps the order of the pages', () => {
    expect(mergePages([page(1, 2), page(3, 2)]).map((row) => row.id)).toEqual([1, 2, 3, 4])
  })

  it('lists a record once when an insert shifted it onto the next page as well', () => {
    expect(mergePages([page(1, 3), page(3, 2)]).map((row) => row.id)).toEqual([1, 2, 3, 4])
  })
})
