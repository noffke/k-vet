import { AlertTriangle, Archive, CircleDashed } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

const badge =
  'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold'

/**
 * "Incomplete — missing: …" for draft records (FR-002). The server reports which
 * mandatory fields are still empty; the labels are translated here.
 */
export function IncompleteBadge({
  missing,
  className,
}: {
  missing: string[] | undefined
  className?: string
}) {
  const { t } = useTranslation()
  if (!missing || missing.length === 0) return null
  const fields = missing.map((field) => t(`field.${toCamelCase(field)}`, field)).join(', ')
  return (
    <span
      className={cn(badge, 'bg-rust-soft text-rust', className)}
      title={t('record.incompleteWithFields', { fields })}
    >
      <CircleDashed className="size-3" aria-hidden />
      {t('record.incomplete')}
    </span>
  )
}

export function ArchivedBadge({ archived }: { archived: boolean | undefined }) {
  const { t } = useTranslation()
  if (!archived) return null
  return (
    <span className={cn(badge, 'bg-sunken text-ink-faint')}>
      <Archive className="size-3" aria-hidden />
      {t('record.archived')}
    </span>
  )
}

/** List-view warning indicator: icon plus the remark as tooltip (FR-007). */
export function WarningIcon({ remark }: { remark: string | null | undefined }) {
  const { t } = useTranslation()
  if (!remark) return null
  return (
    <span
      role="img"
      className={cn(badge, 'bg-blush/50 text-danger')}
      title={remark}
      aria-label={`${t('record.warning')}: ${remark}`}
    >
      <AlertTriangle className="size-3" aria-hidden />
    </span>
  )
}

/** `home_zip` → `homeZip`, so wire field names hit the i18n keys. */
function toCamelCase(field: string): string {
  return field.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
}
