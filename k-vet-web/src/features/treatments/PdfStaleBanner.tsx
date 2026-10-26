import { useNavigate } from '@tanstack/react-router'
import { FileWarning, ReceiptText } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { Treatment } from '@/api/generated/model'
import { Button } from '@/components/ui/button'

/**
 * The invoice PDF no longer shows the treatment: said first on the page, in the warning's
 * colours, with what changed and the way out (issues.md — the note under the invoice panel was
 * easy to overlook). The changed lines and texts below carry a mark of their own.
 */
export function PdfStaleBanner({ treatment }: { treatment: Treatment }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  if (!treatment.pdf_stale) return null

  const changes = treatment.pdf_changes
  const nameOf = (recordId: number) =>
    treatment.patients.find((patient) => patient.id === recordId)?.name ?? ''
  const parts = changes
    ? [
        changes.changed_items.length > 0
          ? t('pdfStale.changedItems', { count: changes.changed_items.length })
          : null,
        changes.removed_items > 0
          ? t('pdfStale.removedItems', { count: changes.removed_items })
          : null,
        changes.changed_records.length > 0
          ? t('pdfStale.changedRecords', {
              names: changes.changed_records.map(nameOf).filter(Boolean).join(', '),
            })
          : null,
        changes.removed_records > 0
          ? t('pdfStale.removedRecords', { count: changes.removed_records })
          : null,
        changes.reordered ? t('pdfStale.reordered') : null,
      ].filter((part): part is string => part !== null)
    : []

  return (
    <aside
      role="alert"
      className="mb-4 flex flex-wrap items-start gap-3 rounded-card border-2 border-danger/40 bg-blush/40 px-4 py-3"
    >
      <FileWarning className="mt-0.5 size-5 shrink-0 text-danger" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-xs font-bold uppercase tracking-wider text-danger">
          {t('pdfStale.title')}
        </p>
        <p className="mt-0.5 text-sm font-medium text-ink">{t('pdfStale.body')}</p>
        {parts.length > 0 ? <p className="mt-0.5 text-sm text-ink">{parts.join(' · ')}</p> : null}
      </div>
      {treatment.invoice && treatment.invoice.status !== 'cancelled' ? (
        <Button
          variant="primary"
          onClick={() =>
            void navigate({ to: '/treatments/$id/invoice', params: { id: String(treatment.id) } })
          }
        >
          <ReceiptText className="size-4" />
          {t('invoices.update')}
        </Button>
      ) : null}
    </aside>
  )
}
