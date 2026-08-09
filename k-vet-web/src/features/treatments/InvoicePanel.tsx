import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Ban, FileText, Mail, ReceiptText } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  getInvoicePdfUrl,
  useCancelInvoice,
  useGetInvoice,
  useSendInvoice,
} from '@/api/generated/endpoints'
import type { Treatment } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useLocaleFormat } from '@/lib/locale'

interface InvoicePanelProps {
  treatment: Treatment
  hasItems: boolean
}

/**
 * Create → inspect the PDF → accept and email. The panel also carries the two warnings
 * that matter: updating an accepted invoice burns its number, and cancelling is final.
 */
export function InvoicePanel({ treatment, hasItems }: InvoicePanelProps) {
  const { t } = useTranslation()
  const { money, date } = useLocaleFormat()
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

  const sendInvoice = useSendInvoice({ mutation: { onSuccess: refresh } })
  const cancelInvoice = useCancelInvoice({
    mutation: {
      onSuccess: async () => {
        setCancelOpen(false)
        await refresh()
      },
    },
  })

  const status = treatment.invoice?.status
  const isAccepted = status === 'accepted' || status === 'submitted'
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
            <p className="text-sm text-ink-soft">{t('save.hint')}</p>
          )}
          {invoice.data ? (
            <p className="mt-0.5 text-xs text-ink-faint">
              {date(invoice.data.invoice_date)} · {money(invoice.data.total_gross)}
              {invoice.data.ts_sent_email ? ` · ${t('invoices.send')} ✓` : ''}
            </p>
          ) : null}
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

          {isAccepted ? (
            <Button
              onClick={() => treatment.invoice && sendInvoice.mutate({ id: treatment.invoice.id })}
              disabled={sendInvoice.isPending || invoice.data?.email_recipients.length === 0}
            >
              <Mail className="size-4" />
              {t('invoices.send')}
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

      {isAccepted ? (
        <p className="mt-3 text-xs text-rust">{t('invoices.acceptedWillCancel')}</p>
      ) : null}

      {/* The PDF is rendered once, when the invoice is created; the totals are recomputed on
          every read. So the document on file can quietly stop matching what is billed. */}
      {treatment.pdf_stale ? (
        <p className="mt-3 rounded-card border border-rust/30 bg-rust-soft/40 px-3 py-2 text-sm text-rust">
          {t('invoices.pdfStale')}
        </p>
      ) : null}

      <Dialog
        open={cancelOpen}
        onOpenChange={setCancelOpen}
        title={t('invoices.cancel')}
        description={t('invoices.cancelConfirm')}
        footer={
          <>
            <Button onClick={() => setCancelOpen(false)}>{t('action.cancel')}</Button>
            <Button
              variant="danger"
              disabled={cancelInvoice.isPending}
              onClick={() =>
                treatment.invoice && cancelInvoice.mutate({ id: treatment.invoice.id })
              }
            >
              {t('action.confirm')}
            </Button>
          </>
        }
      >
        <p className="text-sm text-ink-soft">{t('invoices.cancelConfirm')}</p>
      </Dialog>
    </section>
  )
}
