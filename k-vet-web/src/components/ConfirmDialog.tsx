import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'

interface ConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  /** A subtitle under the title — an invoice number, say, so the right one is obvious. */
  description?: string | undefined
  /** What is about to happen, said plainly. Naming the record is worth the words. */
  children: ReactNode
  /** Defaults to a neutral "Confirm"; deletions pass `action.delete`. */
  confirmLabel?: string | undefined
  /**
   * `danger` for anything that cannot be undone. Archiving is reversible, so it stays
   * neutral — colouring every confirmation red teaches the vet to click through them.
   */
  variant?: 'danger' | 'primary' | undefined
  busy?: boolean | undefined
  onConfirm: () => void
}

/**
 * Asks before something destructive happens.
 *
 * Every delete and archive in the app goes through this rather than firing on the first
 * click, so a mis-tap on a phone during a house call cannot throw away a treatment
 * (issues.md 2). It is the `Dialog` primitive with the footer fixed: cancel on the left,
 * the deed on the right, and the dialog closes itself once the deed is done.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  confirmLabel,
  variant = 'danger',
  busy,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation()
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>{t('action.cancel')}</Button>
          <Button
            variant={variant}
            disabled={busy}
            onClick={() => {
              onConfirm()
              onOpenChange(false)
            }}
          >
            {confirmLabel ?? t('action.confirm')}
          </Button>
        </>
      }
    >
      <div className="text-sm text-ink-soft">{children}</div>
    </Dialog>
  )
}
