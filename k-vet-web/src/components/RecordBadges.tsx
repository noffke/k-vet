import {
  AlertTriangle,
  Archive,
  CircleDashed,
  FileWarning,
  MailWarning,
  ReceiptText,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { InvoiceStatus } from '@/api/generated/model'
import { cn, toCamelCase } from '@/lib/utils'

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

/**
 * "Invoice data missing — …": the customer is a complete record, but an invoice for them would
 * go out without a first name or an address (issues.md 20). Deliberately separate from
 * {@link IncompleteBadge}: recording a customer and billing one ask for different sets, and the
 * database's completeness constraint stays the looser of the two.
 */
export function NotInvoiceableBadge({
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
      className={cn(badge, 'bg-blush/50 text-danger', className)}
      title={t('record.notInvoiceableWithFields', { fields })}
    >
      <FileWarning className="size-3" aria-hidden />
      {t('record.notInvoiceable')}
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

/**
 * Work that was done and billed to nobody — the first of the two ways money goes missing
 * (FR-036). Rust, like the other "unfinished" markers: it is a task, not a hazard.
 */
export function NotBilledBadge({ count }: { count: number | undefined }) {
  const { t } = useTranslation()
  if (!count) return null
  return (
    <span className={cn(badge, 'bg-rust-soft text-rust')}>
      <ReceiptText className="size-3" aria-hidden />
      {t('record.notBilled')}
    </span>
  )
}

/**
 * The second one: a released invoice the customer never got. Only `accepted` can be unsent —
 * sending is what moves an invoice on, and nothing reaches bookkeeping without it.
 */
export function NotSentBadge({ status }: { status: InvoiceStatus | undefined }) {
  const { t } = useTranslation()
  if (status !== 'accepted') return null
  return (
    <span className={cn(badge, 'bg-rust-soft text-rust')}>
      <MailWarning className="size-3" aria-hidden />
      {t('record.notSent')}
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
