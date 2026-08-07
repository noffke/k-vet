import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { useState } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { NumberInput } from '@/components/NumberInput'
import i18n from '@/lib/i18n'

/**
 * Money is formatted in the operator's configured currency, so anything that formats needs a
 * query client. Nothing is fetched here — the hook falls back while the request is pending.
 */
function withQuery(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return <QueryClientProvider client={client}>{ui}</QueryClientProvider>
}

function Harness({
  onChange,
  initial,
  unit,
}: {
  onChange: (value: string | null) => void
  initial?: string
  unit?: string
}) {
  const [value, setValue] = useState<string | null>(initial ?? null)
  return (
    <NumberInput
      label="Preis"
      value={value}
      unit={unit}
      onChange={(next) => {
        setValue(next)
        onChange(next)
      }}
    />
  )
}

const renderHarness = (ui: ReactNode) => render(withQuery(ui))

describe('NumberInput', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('de-DE')
  })

  it('accepts a decimal comma in de-DE and emits a dot-decimal wire value', async () => {
    const onChange = vi.fn()
    renderHarness(<Harness onChange={onChange} />)

    await userEvent.type(screen.getByLabelText('Preis'), '12,5')

    expect(onChange).toHaveBeenLastCalledWith('12.50')
  })

  it('accepts a decimal point in en-US', async () => {
    await i18n.changeLanguage('en-US')
    const onChange = vi.fn()
    renderHarness(<Harness onChange={onChange} />)

    await userEvent.type(screen.getByLabelText('Preis'), '12.5')

    expect(onChange).toHaveBeenLastCalledWith('12.50')
  })

  it('shows an error for text that is not a number and keeps the stored value', async () => {
    const onChange = vi.fn()
    renderHarness(<Harness onChange={onChange} initial="10.00" />)
    const input = screen.getByLabelText('Preis')

    await userEvent.clear(input)
    await userEvent.type(input, 'abc')

    expect(await screen.findByRole('alert')).toHaveTextContent('Keine gültige Zahl')
    // `clear` legitimately reports an empty field; "abc" must not be propagated.
    expect(onChange).toHaveBeenLastCalledWith(null)
  })

  it('shows a unit beside the value without putting it into the value', async () => {
    const onChange = vi.fn()
    renderHarness(<Harness onChange={onChange} initial="20.54" unit="€" />)
    const input = screen.getByLabelText<HTMLInputElement>('Preis')

    expect(screen.getByText('€')).toBeInTheDocument()
    expect(input.value).toBe('20,54')
    expect(input).toHaveAccessibleDescription('€')

    await userEvent.clear(input)
    await userEvent.type(input, '30')

    // The unit is decoration; what the field stores stays a bare number.
    expect(onChange).toHaveBeenLastCalledWith('30.00')
  })

  it('renders the stored value grouped for the active locale and reformats on blur', async () => {
    renderHarness(<Harness onChange={() => {}} initial="1234.50" />)
    const input = screen.getByLabelText<HTMLInputElement>('Preis')

    expect(input.value).toBe('1.234,5')

    await userEvent.click(input)
    // While editing, group separators would fight the cursor.
    expect(input.value).toBe('1234,5')

    await userEvent.tab()
    expect(input.value).toBe('1.234,5')
  })
})
