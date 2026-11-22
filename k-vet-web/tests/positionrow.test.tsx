import { act, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { PositionList, PositionRow } from '@/components/PositionRow'
import { ValueCell } from '@/components/ValueCell'
import i18n from '@/lib/i18n'

vi.mock('@/api/generated/endpoints', () => ({ useGetConfig: () => ({ data: undefined }) }))

beforeAll(async () => {
  await i18n.changeLanguage('de')
})

/** A cell whose open state lives where the row keeps it. */
function Cell({
  label = 'Menge',
  newest = false,
  onEnter,
  onChange = () => undefined,
}: {
  label?: string
  newest?: boolean
  onEnter?: () => void
  onChange?: (value: string | null) => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <ValueCell
      label={label}
      display="2 ×"
      value="2.00"
      open={newest || open}
      onOpen={() => setOpen(true)}
      onClose={() => setOpen(false)}
      autoFocus={newest}
      onEnter={onEnter}
      onChange={onChange}
    />
  )
}

function Row({ newest = false, notice }: { newest?: boolean; notice?: string }) {
  return (
    <PositionList label="Positionen">
      <PositionRow
        icon={<span />}
        name="Allgemeine Untersuchung"
        quantity={<Cell newest={newest} />}
        factor={<Cell label="Faktor" newest={newest} />}
        amount="28,11 €"
        notice={notice}
        details={<p>GOT 25 · 19 % USt.</p>}
        removeLabel="Löschen"
        onRemove={() => undefined}
      />
    </PositionList>
  )
}

/**
 * A position is one line — name, its values, its amount, the bin — and opens on demand: a value
 * where it is tapped, the rest behind the name. The line just picked has its values open.
 */
describe('a position row', () => {
  it('is one line of text until something is wanted', () => {
    render(<Row />)
    const row = screen.getByRole('listitem')
    expect(row).toHaveTextContent('Allgemeine Untersuchung')
    expect(row).toHaveTextContent('2 ×')
    expect(row).toHaveTextContent('28,11 €')
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByText('GOT 25 · 19 % USt.')).toBeNull()
    expect(screen.getByRole('button', { name: 'Löschen' })).toBeInTheDocument()
  })

  it('opens a value where it is tapped — the whole cell, number and unit, is the button', () => {
    render(<Row />)
    const quantity = screen.getByRole('button', { name: /^Menge ändern:/ })
    // The number is inside the button, not beside it: tapping it is tapping the button.
    expect(quantity).toHaveTextContent('2 ×')
    fireEvent.click(quantity)
    const field = screen.getByLabelText('Menge')
    expect(field).toHaveFocus()
    // The factor stays text: only what was tapped opens.
    expect(screen.queryByLabelText('Faktor')).toBeNull()

    fireEvent.keyDown(field, { key: 'Escape' })
    act(() => field.blur())
    expect(screen.queryByLabelText('Menge')).toBeNull()
  })

  it('closes a value left by a press only once the press is released', async () => {
    render(<Row />)
    fireEvent.click(screen.getByRole('button', { name: /^Menge ändern:/ }))
    const field = screen.getByLabelText('Menge')
    // Pressed elsewhere: the row must not shrink under the pointer before the release.
    fireEvent.pointerDown(window)
    act(() => field.blur())
    expect(screen.getByLabelText('Menge')).toBeInTheDocument()
    fireEvent.pointerUp(window)
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0))
    })
    expect(screen.queryByLabelText('Menge')).toBeNull()
  })

  it('opens the values of the line just picked and focuses its quantity', () => {
    render(<Row newest />)
    expect(screen.getByLabelText('Menge')).toHaveFocus()
    expect(screen.getByLabelText('Faktor')).toBeInTheDocument()
  })

  it('sends Enter on to where the next position is searched', () => {
    const onEnter = vi.fn()
    render(<Cell newest onEnter={onEnter} />)
    fireEvent.keyDown(screen.getByLabelText('Menge'), { key: 'Enter' })
    expect(onEnter).toHaveBeenCalledTimes(1)
  })

  it('does not take the focus from a field the vet is already typing in', () => {
    render(
      <>
        <input aria-label="Suche" defaultValue="Beratung" />
        <Cell newest />
      </>,
    )
    const search = screen.getByLabelText('Suche')
    act(() => search.focus())
    // The newest line arriving after the vet typed on: it opens, but leaves her typing.
    render(<Cell newest />)
    expect(search).toHaveFocus()
  })

  it('shows the rest behind its name, and says what cannot wait while closed', () => {
    render(<Row notice="Ohne Kilometer kann keine Rechnung erstellt werden." />)
    expect(screen.getByText('Ohne Kilometer kann keine Rechnung erstellt werden.')).toBeVisible()
    const name = screen.getByRole('button', { name: 'Allgemeine Untersuchung' })
    expect(name).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(name)
    expect(name).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('GOT 25 · 19 % USt.')).toBeVisible()
  })
})
