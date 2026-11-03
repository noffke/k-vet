import { Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'

interface DeleteDraftButtonProps {
  /** Named in the confirmation, so the vet sees which record is about to go. */
  name: string
  /** Deletes the record, then leaves the page or refreshes what showed it. */
  onDelete: () => Promise<unknown>
  /** Names the button where "Löschen" alone is ambiguous — on a phone it is an icon. */
  label?: string | undefined
  /** `small` inside a card, such as a packaging on the drug page. */
  size?: 'small' | undefined
}

/**
 * "Löschen" for a record that is still incomplete — the one a stray "Neu" left behind. Pages
 * show it instead of {@link ArchiveButton} while the record is a draft: archiving a draft
 * changes nothing, since drafts are kept out of the pickers and billing anyway. A complete
 * record is archived, never deleted.
 *
 * The server refuses while something still refers to the record (a drug naming the
 * manufacturer, an animal on the customer); that refusal, and any other failure, is said in a
 * dialog of its own rather than lost.
 */
export function DeleteDraftButton({ name, onDelete, label, size }: DeleteDraftButtonProps) {
  const { t } = useTranslation()
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [refusal, setRefusal] = useState<string | null>(null)

  const remove = async () => {
    setBusy(true)
    try {
      await onDelete()
    } catch (error) {
      setRefusal(
        error instanceof ApiError && error.status === 409
          ? t('record.deleteInUse', { name })
          : t('error.generic'),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Button
        variant="danger"
        size={size}
        aria-label={label ?? t('action.delete')}
        onClick={() => setAsking(true)}
        disabled={busy}
      >
        <Trash2 className="size-4" />
        <span className="sr-only sm:not-sr-only">{t('action.delete')}</span>
      </Button>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title={t('confirm.deleteDraftTitle')}
        confirmLabel={t('action.delete')}
        busy={busy}
        onConfirm={() => void remove()}
      >
        {t('confirm.deleteDraftBody', { name })}
      </ConfirmDialog>
      <Dialog
        open={refusal !== null}
        onOpenChange={(open) => {
          if (!open) setRefusal(null)
        }}
        title={t('record.deleteRefused')}
        footer={<Button onClick={() => setRefusal(null)}>{t('action.close')}</Button>}
      >
        <p className="text-sm text-ink-soft">{refusal}</p>
      </Dialog>
    </>
  )
}
