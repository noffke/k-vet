import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
// Real translations rather than keys: a mistyped key then fails the test too.
import '@/lib/i18n'
import { ArchiveButton } from '@/components/ArchiveButton'
import { ConfirmDialog } from '@/components/ConfirmDialog'

describe('ConfirmDialog', () => {
  it('does nothing until the deed is confirmed', async () => {
    const user = userEvent.setup()
    const onConfirm = vi.fn()
    const onOpenChange = vi.fn()

    render(
      <ConfirmDialog
        open
        onOpenChange={onOpenChange}
        title="Position entfernen"
        confirmLabel="Löschen"
        onConfirm={onConfirm}
      >
        Wirklich entfernen?
      </ConfirmDialog>,
    )

    expect(screen.getByText('Wirklich entfernen?')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Abbrechen' }))
    expect(onConfirm).not.toHaveBeenCalled()
    expect(onOpenChange).toHaveBeenCalledWith(false)

    await user.click(screen.getByRole('button', { name: 'Löschen' }))
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('closes itself once the deed is done, so the caller need not', async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    const order: string[] = []

    render(
      <ConfirmDialog
        open
        onOpenChange={(open) => {
          onOpenChange(open)
          order.push(`close:${open}`)
        }}
        title="Stornieren"
        onConfirm={() => order.push('confirm')}
      >
        Rechnung stornieren?
      </ConfirmDialog>,
    )

    await user.click(screen.getByRole('button', { name: 'Bestätigen' }))
    // The deed runs first: closing must not cancel the mutation it just started.
    expect(order).toEqual(['confirm', 'close:false'])
  })
})

describe('ArchiveButton', () => {
  it('asks before archiving, and names the record', async () => {
    const user = userEvent.setup()
    const onArchive = vi.fn()

    render(
      <ArchiveButton
        archived={false}
        name="Bello"
        onArchive={onArchive}
        onUnarchive={() => undefined}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Archivieren' }))
    expect(onArchive).not.toHaveBeenCalled()
    expect(screen.getByText(/Bello/)).toBeInTheDocument()

    const dialog = screen.getByRole('dialog')
    await user.click(within(dialog).getByRole('button', { name: 'Archivieren' }))
    expect(onArchive).toHaveBeenCalledTimes(1)
  })

  it('restores without asking — putting a record back harms nothing', async () => {
    const user = userEvent.setup()
    const onUnarchive = vi.fn()

    render(
      <ArchiveButton archived name="Bello" onArchive={() => undefined} onUnarchive={onUnarchive} />,
    )

    await user.click(screen.getByRole('button', { name: 'Wiederherstellen' }))
    expect(onUnarchive).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('promises no restore where the page cannot offer one', async () => {
    const user = userEvent.setup()

    render(<ArchiveButton archived={false} name="Impfung" onArchive={() => undefined} />)

    await user.click(screen.getByRole('button', { name: 'Archivieren' }))
    expect(screen.getByText(/nicht wieder einblenden/)).toBeInTheDocument()
  })
})
