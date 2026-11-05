import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { ScanField } from '@/components/ScanField'
import i18n from '@/lib/i18n'

/**
 * A barcode scanner set up as a keyboard: it types what it reads and presses Enter. Inside a
 * Data Matrix it sends the group separator as Ctrl+] — on a German layout Ctrl and the "+"
 * key — which must end up in the scan, not zoom the page.
 */
describe('ScanField', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('de-DE')
  })

  it('hands over a scan on Enter and is empty for the next one', async () => {
    const onScan = vi.fn()
    render(<ScanField label="Code" onScan={onScan} />)
    const field = screen.getByLabelText('Code')

    await userEvent.type(field, '4006381333931{Enter}')
    expect(onScan).toHaveBeenCalledWith('4006381333931')
    expect(field).toHaveValue('')
    expect(screen.getByText(/Zuletzt gelesen: 4006381333931/)).toBeInTheDocument()

    // Enter on nothing is no scan.
    await userEvent.type(field, '{Enter}')
    expect(onScan).toHaveBeenCalledTimes(1)
  })

  it('takes Ctrl+] and Ctrl++ as the group separator and shows it', async () => {
    const onScan = vi.fn()
    render(<ScanField label="Code" onScan={onScan} />)
    const field = screen.getByLabelText('Code')

    await userEvent.type(field, '10AB{Control>}]{/Control}21X{Control>}+{/Control}{Enter}')
    expect(onScan).toHaveBeenCalledWith('10AB\u001d21X\u001d')
    expect(screen.getByText(/Zuletzt gelesen: 10AB␝21X␝/)).toBeInTheDocument()
  })
})
