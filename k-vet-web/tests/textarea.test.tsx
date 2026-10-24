import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeAll, describe, expect, it } from 'vitest'
import { TextAreaField } from '@/components/ui/field'

/**
 * jsdom lays nothing out, so `scrollHeight` is 0 for everything and the component would look
 * like it never grows. Stand in for the browser closely enough to be worth testing against:
 * the height the content needs, but **never less than the height already set** — that is the
 * property a real textarea has, and the reason the box has to be collapsed before measuring.
 * Without this second half the shrink test passes whether or not the collapse is there.
 */
const LINE_PX = 20
beforeAll(() => {
  Object.defineProperty(HTMLTextAreaElement.prototype, 'scrollHeight', {
    configurable: true,
    get(this: HTMLTextAreaElement) {
      const content = (this.value.split('\n').length + 1) * LINE_PX
      const set = Number.parseInt(this.style.height, 10)
      return Number.isNaN(set) ? content : Math.max(content, set)
    },
  })
})

describe('TextAreaField', () => {
  it('opens at the height of the text it is given', () => {
    render(<TextAreaField label="Vorbericht" defaultValue={'eins\nzwei\ndrei\nvier'} />)
    const field = screen.getByLabelText('Vorbericht')
    expect(field.style.height).toBe(`${5 * LINE_PX}px`)
  })

  it('grows while typing and shrinks again when the text goes', async () => {
    const user = userEvent.setup()
    render(<TextAreaField label="Befund" defaultValue="" />)
    const field = screen.getByLabelText('Befund') as HTMLTextAreaElement

    const initial = field.style.height
    await user.type(field, 'eins{Enter}zwei{Enter}drei')
    const grown = field.style.height
    expect(Number.parseInt(grown, 10)).toBeGreaterThan(Number.parseInt(initial, 10))

    await user.clear(field)
    // Collapsing first is what lets it come back down; without it the box only ever grows.
    expect(Number.parseInt(field.style.height, 10)).toBeLessThan(Number.parseInt(grown, 10))
  })
})
