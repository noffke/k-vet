import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { MailX, RotateCcw, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  getGetDashboardQueryKey,
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  useRemoveQueuedMail,
  useRetryQueuedMail,
} from '@/api/generated/endpoints'
import type { FailedMail } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Every invoice mail that failed on the way out (migration 0026): still being retried, or given
 * up. Each needs the vet's decision — try again, or take it out of the queue and get the
 * invoice to the customer another way — so all of them are listed, not the oldest few.
 */
export function FailedMailsCard({ mails }: { mails: FailedMail[] | undefined }) {
  const { t } = useTranslation()
  const failed = mails ?? []

  return (
    <section className="rounded-card border border-line bg-surface p-4 sm:col-span-2 lg:col-span-3">
      <h2 className="eyebrow flex items-center gap-1.5">
        <MailX className="size-3.5" />
        {t('dashboard.failedMails')}
        {failed.length > 0 ? (
          <span className="numeric ml-auto rounded-full bg-blush/60 px-2 py-0.5 text-xs font-bold text-danger">
            {failed.length}
          </span>
        ) : null}
      </h2>
      {failed.length === 0 ? (
        <p className="mt-2 text-sm text-ink-soft">{t('dashboard.noFailedMails')}</p>
      ) : (
        <>
          <p className="mt-1 text-xs text-ink-faint">{t('dashboard.failedMailsHint')}</p>
          <ul className="mt-1 flex flex-col divide-y divide-line">
            {failed.map((entry) => (
              <FailedMailRow key={entry.mail.id} entry={entry} />
            ))}
          </ul>
        </>
      )}
    </section>
  )
}

function FailedMailRow({ entry }: { entry: FailedMail }) {
  const { t } = useTranslation()
  const { dateTime } = useLocaleFormat()
  const client = useQueryClient()
  const { mail } = entry

  // The card, the invoice and the appointment it belongs to all show the queued mail.
  const refresh = async () => {
    await client.invalidateQueries({ queryKey: getGetDashboardQueryKey() })
    await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(mail.invoice_id) })
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(entry.treatment_id) })
  }
  const retry = useRetryQueuedMail({
    mutation: {
      onSuccess: async () => {
        toast(t('mailQueue.retried'))
        await refresh()
      },
    },
  })
  const remove = useRemoveQueuedMail({
    mutation: {
      onSuccess: async () => {
        toast(t('mailQueue.removed'))
        await refresh()
      },
    },
  })
  const busy = retry.isPending || remove.isPending

  const state = mail.refused
    ? t('mailQueue.refused')
    : mail.gave_up_at
      ? t('mailQueue.gaveUp')
      : t('mailQueue.retrying', { time: dateTime(mail.retry_until) })

  return (
    <li className="flex flex-col gap-2 py-3 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
      <div className="min-w-0">
        <Link
          to="/treatments/$id"
          params={{ id: String(entry.treatment_id) }}
          className="text-sm text-ink hover:text-rust"
        >
          {entry.customer_name}
          <span className="numeric text-ink-faint"> · {entry.invoice_number}</span>
        </Link>
        <p className="text-xs break-all text-ink-soft">
          {t('invoices.sentTo', { recipients: mail.recipients.join(', ') })}
        </p>
        <p className="mt-1 text-xs font-semibold text-danger">{state}</p>
        {/* The transport's own words: what the mail server answered, or why it could not be
            reached. English, but the part someone fixing the setup needs. */}
        <p className="text-xs break-all text-ink-faint">{mail.last_error}</p>
      </div>
      <div className="flex shrink-0 flex-wrap gap-2">
        <Button disabled={busy} onClick={() => retry.mutate({ id: mail.id })}>
          <RotateCcw className="size-4" />
          {t('mailQueue.retry')}
        </Button>
        <Button
          variant="ghost"
          disabled={busy}
          aria-label={t('mailQueue.removeLabel')}
          onClick={() => remove.mutate({ id: mail.id })}
        >
          <Trash2 className="size-4" />
          {t('mailQueue.remove')}
        </Button>
      </div>
    </li>
  )
}
