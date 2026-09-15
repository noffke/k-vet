import { Archive, ArchiveRestore } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'

interface ArchiveButtonProps {
  archived: boolean
  /** Named in the confirmation, so the vet sees which record is about to disappear. */
  name: string
  onArchive: () => void
  /**
   * Left out where the page offers no way back — the confirmation then says so instead of
   * promising a restore that is not there.
   */
  onUnarchive?: (() => void) | undefined
  busy?: boolean | undefined
}

/**
 * The archive/unarchive toggle that sits in every master-data page header.
 *
 * It was the same eleven lines copied into seven pages, none of which asked first
 * (issues.md 2). Archiving is the soft delete for master data — the record leaves search
 * and the pickers but keeps every invoice that references it — so the confirmation stays
 * neutral rather than red, and restoring needs no confirmation at all.
 */
export function ArchiveButton({
  archived,
  name,
  onArchive,
  onUnarchive,
  busy,
}: ArchiveButtonProps) {
  const { t } = useTranslation()
  const [asking, setAsking] = useState(false)

  if (archived) {
    // Nothing to offer when the page cannot restore: the record is already out of the way.
    if (!onUnarchive) return null
    return (
      <Button onClick={onUnarchive} disabled={busy}>
        <ArchiveRestore className="size-4" />
        <span className="sr-only sm:not-sr-only">{t('record.unarchive')}</span>
      </Button>
    )
  }

  return (
    <>
      <Button onClick={() => setAsking(true)} disabled={busy}>
        <Archive className="size-4" />
        <span className="sr-only sm:not-sr-only">{t('record.archive')}</span>
      </Button>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title={t('confirm.archiveTitle')}
        confirmLabel={t('record.archive')}
        variant="primary"
        busy={busy}
        onConfirm={onArchive}
      >
        {onUnarchive
          ? t('confirm.archiveBody', { name })
          : t('confirm.archiveBodyNoRestore', { name })}
      </ConfirmDialog>
    </>
  )
}
