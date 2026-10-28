import { act, render, screen } from '@testing-library/react'
import { Command } from 'cmdk'
import { beforeAll, describe, expect, it } from 'vitest'
import { unmarkCmdkInput } from '@/lib/cmdk'

function Picker({ label, items, unmark }: { label: string; items: string[]; unmark: boolean }) {
  return (
    <Command shouldFilter={false} label={label}>
      <Command.Input placeholder={label} ref={unmark ? unmarkCmdkInput : undefined} />
      <Command.List>
        {items.map((item) => (
          <Command.Item key={item} value={item}>
            {item}
          </Command.Item>
        ))}
      </Command.List>
    </Command>
  )
}

/**
 * Two pickers on one page — the Preisauskunft's customer and position search. cmdk moved the
 * focus to a picker's own input whenever its selection changed while *any* picker had focus,
 * so the customer list arriving late pulled the vet's typing out of the position search and
 * into the customer field. `unmarkCmdkInput` keeps each picker to itself.
 */
describe('cmdk pickers side by side', () => {
  beforeAll(() => {
    // jsdom has neither; cmdk measures its list and scrolls the selected item into view.
    globalThis.ResizeObserver ??= class {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    Element.prototype.scrollIntoView ??= () => {}
  })

  async function typeWhileTheOtherFills(unmark: boolean): Promise<HTMLElement> {
    const { rerender } = render(
      <>
        <Picker label="Kunde" items={[]} unmark={unmark} />
        <Picker label="Position" items={['Untersuchung']} unmark={unmark} />
      </>,
    )
    const typing = screen.getByPlaceholderText('Position')
    act(() => typing.focus())

    rerender(
      <>
        <Picker label="Kunde" items={['Erika Mustermann', 'Max Mustermann']} unmark={unmark} />
        <Picker label="Position" items={['Untersuchung']} unmark={unmark} />
      </>,
    )
    await act(async () => {})
    return typing
  }

  it('leave the focus where the vet is typing when another picker fills', async () => {
    const typing = await typeWhileTheOtherFills(true)
    expect(document.activeElement).toBe(typing)
  })

  it('still need it: cmdk alone pulls the focus (drop unmarkCmdkInput once this fails)', async () => {
    await typeWhileTheOtherFills(false)
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Kunde'))
  })
})
