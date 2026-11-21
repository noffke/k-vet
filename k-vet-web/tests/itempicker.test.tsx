import { act, fireEvent, render, screen } from '@testing-library/react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { PickerItem } from '@/api/generated/model'
import { ItemPicker } from '@/components/ItemPicker'
import i18n from '@/lib/i18n'

const items = [
  {
    kind: 'service',
    id: 1,
    name: 'Allgemeine Untersuchung',
    price_gross: '28.11',
    per_visit: false,
  },
  { kind: 'service', id: 2, name: 'Impfung', price_gross: '12.00', per_visit: false },
] as unknown as PickerItem[]

vi.mock('@/api/generated/endpoints', () => ({
  usePickerItems: () => ({ data: items, isPending: false }),
  useListTemplates: () => ({ data: [] }),
  useGetConfig: () => ({ data: undefined }),
}))

beforeAll(async () => {
  await i18n.changeLanguage('de')
  // jsdom has neither; cmdk measures its list and scrolls the selected item into view.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Element.prototype.scrollIntoView ??= () => {}
})

const field = () => screen.getByPlaceholderText('Medikament oder Leistung suchen …')

/**
 * The picker sits under the list it adds to, so at rest it is only its search field: the
 * results open while it has the focus and close on Escape or when the focus leaves.
 */
describe('the item picker', () => {
  it('is only its search field at rest', () => {
    render(<ItemPicker onPick={() => undefined} />)
    expect(screen.queryAllByRole('option')).toHaveLength(0)
    expect(field()).toHaveAttribute('aria-expanded', 'false')
  })

  it('opens its results with the focus and closes them on Escape or when it is left', () => {
    render(<ItemPicker onPick={() => undefined} />)
    act(() => field().focus())
    expect(screen.getAllByRole('option')).toHaveLength(2)
    expect(field()).toHaveAttribute('aria-expanded', 'true')

    fireEvent.keyDown(field(), { key: 'Escape' })
    expect(screen.queryAllByRole('option')).toHaveLength(0)

    // Typing opens them again.
    fireEvent.change(field(), { target: { value: 'Imp' } })
    expect(screen.getAllByRole('option')).toHaveLength(2)

    act(() => field().blur())
    expect(screen.queryAllByRole('option')).toHaveLength(0)
  })

  it('picks with a click and stays open for the next one', () => {
    const onPick = vi.fn()
    render(<ItemPicker onPick={onPick} />)
    act(() => field().focus())
    const option = screen.getByRole('option', { name: /Impfung/ })
    // The press must not take the focus off the field, or the list would close first.
    expect(fireEvent.mouseDown(option)).toBe(false)
    fireEvent.click(option)
    expect(onPick).toHaveBeenCalledWith(expect.objectContaining({ id: 2 }))
    expect(screen.getAllByRole('option')).toHaveLength(2)
  })

  it('takes no pick while one is on its way, but keeps the field', () => {
    const onPick = vi.fn()
    render(<ItemPicker onPick={onPick} disabled />)
    expect(field()).toBeEnabled()
    act(() => field().focus())
    fireEvent.click(screen.getByRole('option', { name: /Impfung/ }))
    expect(onPick).not.toHaveBeenCalled()
  })
})
