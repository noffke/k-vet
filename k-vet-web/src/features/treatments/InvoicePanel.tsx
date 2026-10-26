import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { Ban, FileText, Mail, ReceiptText, Send } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  getInvoicePdfUrl,
  useCancelInvoice,
  useGetInvoice,
  useListEmailTemplates,
  useMarkInvoicePosted,
} from '@/api/generated/endpoints'
import type { QueuedEmail, Treatment } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { NotSentBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { SentTo } from '@/features/invoices/SentTo'
import { useLocaleFormat } from '@/lib/locale'
import { isAhead } from '@/lib/schedule'

interface InvoicePanelProps {
  treatment: Treatment
  hasItems: boolean
}

/**
 * Create → inspect the PDF → release → get it to the customer, by e-mail or on paper. The
 * panel also carries the two warnings that matter: updating a released invoice burns its
 * number, and cancelling is final.
 */
export function InvoicePanel({ treatment, hasItems }: InvoicePanelProps) {
  const { t } = useTranslation()
  const { money, date, dateTime } = useLocaleFormat()
  const client = useQueryClient()
  const navigate = useNavigate()

  const [cancelOpen, setCancelOpen] = useState(false)

  const invoiceId = treatment.invoice?.id
  const invoice = useGetInvoice(invoiceId ?? 0, { query: { enabled: Boolean(invoiceId) } })

  const refresh = async () => {
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
    if (invoiceId) {
      await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(invoiceId) })
    }
  }

  const markPosted = useMarkInvoicePosted({ mutation: { onSuccess: refresh } })
  const cancelInvoice = useCancelInvoice({
    mutation: {
      onSuccess: async () => {
        setCancelOpen(false)
        // A cancellation reaches past this page — the dashboard's planned mails, the invoice
        // lists, the stock it gives back — so everything is read afresh when next shown.
        await client.invalidateQueries()
      },
    },
  })

  const status = treatment.invoice?.status
  const isReleased = status === 'accepted' || status === 'sent' || status === 'submitted'
  // Released and still with the practice: the one stage where the postal mark makes sense.
  const isUnsent = status === 'accepted'
  // A cancelled invoice is history: the panel shows it, but the actions start over.
  const isCancelled = status === 'cancelled'
  const live = treatment.invoice && !isCancelled ? treatment.invoice : null

  return (
    <section className="mt-6 rounded-card border border-line bg-cream-soft p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p className="eyebrow">{t('invoices.title')}</p>
          {treatment.invoice ? (
            <p className="numeric text-base font-semibold text-ink">
              {treatment.invoice.invoice_number}
              <span className="ml-2 rounded-full bg-surface px-2 py-0.5 text-xs font-medium text-ink-soft">
                {t(`invoices.status_${treatment.invoice.status}`)}
              </span>
            </p>
          ) : (
            <p className="text-sm text-ink-soft">{t('invoices.noInvoice')}</p>
          )}
          {invoice.data ? (
            <p className="mt-0.5 text-xs text-ink-faint">
              {date(invoice.data.invoice_date)} · {money(invoice.data.total_gross)}
              {/* Which route it took, and when — the `sent` status alone does not say. */}
              {invoice.data.ts_sent_email
                ? ` · ${t('invoices.sentByEmail')} ${dateTime(invoice.data.ts_sent_email)}`
                : ''}
              {invoice.data.ts_sent_post
                ? ` · ${t('invoices.sentByPost')} ${dateTime(invoice.data.ts_sent_post)}`
                : ''}
            </p>
          ) : null}
          {invoice.data?.ts_sent_email ? (
            <SentTo
              className="mt-0.5 block"
              to={invoice.data.sent_email_to}
              cc={invoice.data.sent_email_cc}
              bcc={invoice.data.sent_email_bcc}
              template={invoice.data.sent_email_template}
            />
          ) : null}
          {isUnsent ? (
            <p className="mt-1.5">
              <NotSentBadge status={status} />
            </p>
          ) : null}
          {invoice.data?.email_queue ? <QueuedMailNote mail={invoice.data.email_queue} /> : null}
        </div>

        <div className="flex flex-wrap gap-2">
          {treatment.invoice ? (
            <a
              href={getInvoicePdfUrl(treatment.invoice.id)}
              target="_blank"
              rel="noreferrer"
              className="inline-flex min-h-11 items-center gap-2 rounded-control border border-line-strong bg-surface px-4 text-sm text-ink hover:bg-sunken sm:min-h-9"
            >
              <FileText className="size-4" />
              {t('invoices.pdf')}
            </a>
          ) : null}

          <Button
            variant="accent"
            disabled={!hasItems}
            onClick={() =>
              void navigate({
                to: '/treatments/$id/invoice',
                params: { id: String(treatment.id) },
              })
            }
          >
            <ReceiptText className="size-4" />
            {live ? t('invoices.update') : t('invoices.create')}
          </Button>

          {status === 'created' ? (
            <Button
              variant="primary"
              onClick={() =>
                void navigate({
                  to: '/treatments/$id/invoice/accept',
                  params: { id: String(treatment.id) },
                })
              }
            >
              <Mail className="size-4" />
              {t('invoices.accept')}
            </Button>
          ) : null}

          {isReleased ? (
            <Button
              onClick={() =>
                void navigate({
                  to: '/treatments/$id/invoice/send',
                  params: { id: String(treatment.id) },
                })
              }
            >
              <Mail className="size-4" />
              {t('invoices.send')}
            </Button>
          ) : null}

          {/* Nothing else can see a letter go into a postbox, so the vet says so. */}
          {isUnsent ? (
            <Button
              variant="primary"
              title={t('invoices.markPostedHint')}
              disabled={markPosted.isPending}
              onClick={() => treatment.invoice && markPosted.mutate({ id: treatment.invoice.id })}
            >
              <Send className="size-4" />
              {t('invoices.markPosted')}
            </Button>
          ) : null}

          {live ? (
            <Button variant="danger" onClick={() => setCancelOpen(true)}>
              <Ban className="size-4" />
              {t('invoices.cancel')}
            </Button>
          ) : null}
        </div>
      </div>

      {isReleased ? (
        <p className="mt-3 text-xs text-rust">{t('invoices.acceptedWillCancel')}</p>
      ) : null}

      {/* The PDF is rendered once, when the invoice is created; the totals are recomputed on
          every read. So the document on file can quietly stop matching what is billed. */}
      {treatment.pdf_stale ? (
        <p className="mt-3 rounded-card border border-rust/30 bg-rust-soft/40 px-3 py-2 text-sm text-rust">
          {t('invoices.pdfStale')}
        </p>
      ) : null}

      {/* The number is burned on cancellation, so name the invoice being cancelled. */}
      <ConfirmDialog
        open={cancelOpen}
        onOpenChange={setCancelOpen}
        title={t('invoices.cancel')}
        description={treatment.invoice?.invoice_number}
        busy={cancelInvoice.isPending}
        onConfirm={() => {
          if (treatment.invoice) cancelInvoice.mutate({ id: treatment.invoice.id })
        }}
      >
        {t('invoices.cancelConfirm')}
        {invoice.data?.email_queue ? ` ${t('invoices.cancelDropsMail')}` : ''}
      </ConfirmDialog>
    </section>
  )
}

/**
 * Where the invoice's mail is: waiting for the queue's next run, failing and tried again, or
 * given up — then the dashboard is where it is retried or removed. No request sends mail any
 * more, so this is the only place an appointment says what became of it.
 */
function QueuedMailNote({ mail }: { mail: QueuedEmail }) {
  const { t } = useTranslation()
  const { dateTime } = useLocaleFormat()
  const templates = useListEmailTemplates()
  // Which text is about to go out — worth saying only where there is more than one.
  const templateLine =
    (templates.data?.length ?? 0) > 1 && mail.template ? (
      <span className="block">{t('invoices.queueTemplate', { name: mail.template })}</span>
    ) : null
  if (!mail.last_error) {
    const recipients = mail.recipients.join(', ')
    return (
      <p className="mt-1 text-xs break-all text-ink-soft">
        {isAhead(mail.send_after)
          ? t('invoices.queueScheduled', { recipients, time: dateTime(mail.send_after) })
          : t('invoices.queueWaiting', { recipients })}
        {templateLine}
      </p>
    )
  }
  const stopped = mail.refused || mail.gave_up_at
  return (
    <p className="mt-1 text-xs break-all text-danger">
      {stopped
        ? t('invoices.queueStopped', { error: mail.last_error })
        : t('invoices.queueFailing', {
            error: mail.last_error,
            time: dateTime(mail.retry_until),
          })}
      {stopped ? (
        <>
          {' · '}
          <Link to="/" className="underline hover:text-rust">
            {t('nav.dashboard')}
          </Link>
        </>
      ) : null}
    </p>
  )
}
