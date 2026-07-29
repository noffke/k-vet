import { AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

interface WarningBannerProps {
  /** Whose warning it is — a customer or a patient name. */
  title?: string
  remark: string
  className?: string
}

/**
 * Warning remarks are the one thing that must be seen before anything else on a record
 * (FR-007): a dangerous animal is a safety matter, not a note.
 */
export function WarningBanner({ title, remark, className }: WarningBannerProps) {
  const { t } = useTranslation()
  if (!remark) return null
  return (
    <aside
      role="alert"
      className={cn(
        'flex items-start gap-3 rounded-card border-2 border-danger/40 bg-blush/40 px-4 py-3',
        className,
      )}
    >
      <AlertTriangle className="mt-0.5 size-5 shrink-0 text-danger" aria-hidden />
      <div className="min-w-0">
        <p className="text-xs font-bold uppercase tracking-wider text-danger">
          {title ? `${t('record.warning')} · ${title}` : t('record.warning')}
        </p>
        <p className="mt-0.5 text-sm font-medium text-ink">{remark}</p>
      </div>
    </aside>
  )
}
