import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

/**
 * Where an emailed invoice went: the addresses the mail server took, and the template the mail
 * was written with, recorded together with the send time — so a resend that never got through
 * does not show up here (issues.md, 2026-10-17). Draws nothing for an invoice no mail has left
 * with. Addresses wrap rather than push a phone's screen sideways.
 */
export function SentTo({
  to,
  cc = [],
  bcc = [],
  template,
  className,
}: {
  to: string[]
  cc?: string[]
  bcc?: string[]
  /** Empty for invoices emailed before templates could be chosen. */
  template?: string | null | undefined
  className?: string
}) {
  const { t } = useTranslation()
  if (to.length === 0) return null

  const parts = [
    t('invoices.sentTo', { recipients: to.join(', ') }),
    cc.length > 0 ? t('invoices.sentCc', { recipients: cc.join(', ') }) : null,
    bcc.length > 0 ? t('invoices.sentBcc', { recipients: bcc.join(', ') }) : null,
    template ? t('invoices.sentTemplate', { name: template }) : null,
  ].filter(Boolean)

  return (
    <span className={cn('text-xs break-all text-ink-faint', className)}>{parts.join(' · ')}</span>
  )
}
