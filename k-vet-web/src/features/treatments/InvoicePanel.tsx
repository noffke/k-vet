import { useQueryClient } from '@tanstack/react-query'
import { Ban, FileText, Mail, ReceiptText } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  getInvoicePdfUrl,
  useAcceptInvoice,
  useCancelInvoice,
  useCreateInvoice,
  useGetInvoice,
  useSendInvoice,
} from '@/api/generated/endpoints'
import type { Treatment } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { CheckboxField, TextAreaField, TextField } from '@/components/ui/field'
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

  const [createOpen, setCreateOpen] = useState(false)
  const [acceptOpen, setAcceptOpen] = useState(false)
  const [cancelOpen, setCancelOpen] = useState(false)
  const [includesFinding, setIncludesFinding] = useState(false)
  const [note, setNote] = useState('')
  const [recipients, setRecipients] = useState<string[]>([])
  const [extraRecipient, setExtraRecipient] = useState('')
  // Opened empty on the click and pointed at the PDF when it exists: a tab opened from an
  // awaited mutation is a popup, and every browser blocks that.
  const pdfTab = useRef<Window | null>(null)

  const invoiceId = treatment.invoice?.id
  const invoice = useGetInvoice(invoiceId ?? 0, { query: { enabled: Boolean(invoiceId) } })

  const refresh = async () => {
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
    if (invoiceId) {
      await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(invoiceId) })
    }
  }

  const createInvoice = useCreateInvoice({
    mutation: {
      onSuccess: async (created) => {
        setCreateOpen(false)
        if (pdfTab.current && !pdfTab.current.closed) {
          pdfTab.current.location.href = getInvoicePdfUrl(created.id)
        }
        pdfTab.current = null
        await refresh()
      },
      onError: () => {
        pdfTab.current?.close()
        pdfTab.current = null
      },
    },
  })
  const acceptInvoice = useAcceptInvoice({
    mutation: {
      onSuccess: async () => {
        setAcceptOpen(false)
        await refresh()
      },
    },
  })
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

  const openCreateDialog = () => {
    // A new invoice lists the finding; an existing one opens with what it was created with.
    setIncludesFinding(invoice.data?.includes_finding ?? true)
    setNote(invoice.data?.note ?? '')
    setCreateOpen(true)
  }

  const openAcceptDialog = () => {
    setRecipients(treatment.customer_emails)
    setAcceptOpen(true)
  }

  const toggleRecipient = (email: string) =>
    setRecipients((current) =>
      current.includes(email) ? current.filter((entry) => entry !== email) : [...current, email],
    )

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

          <Button variant="accent" onClick={openCreateDialog} disabled={!hasItems}>
            <ReceiptText className="size-4" />
            {live ? t('invoices.update') : t('invoices.create')}
          </Button>

          {status === 'created' ? (
            <Button variant="primary" onClick={openAcceptDialog}>
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
        open={createOpen}
        onOpenChange={setCreateOpen}
        title={live ? t('invoices.update') : t('invoices.create')}
        description={isAccepted ? t('invoices.acceptedWillCancel') : undefined}
        footer={
          <>
            <Button onClick={() => setCreateOpen(false)}>{t('action.cancel')}</Button>
            <Button
              variant="primary"
              disabled={createInvoice.isPending}
              onClick={() => {
                pdfTab.current = window.open('', '_blank')
                createInvoice.mutate({
                  id: treatment.id,
                  data: { includes_finding: includesFinding, note: note || null },
                })
              }}
            >
              {live ? t('invoices.update') : t('invoices.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <CheckboxField
            label={t('invoices.includesFinding')}
            checked={includesFinding}
            onChange={(event) => setIncludesFinding(event.target.checked)}
          />
          <TextAreaField
            label={t('field.note')}
            value={note}
            onChange={(event) => setNote(event.target.value)}
          />
        </div>
      </Dialog>

      <Dialog
        open={acceptOpen}
        onOpenChange={setAcceptOpen}
        title={t('invoices.accept')}
        description={t('invoices.recipients')}
        footer={
          <>
            <Button onClick={() => setAcceptOpen(false)}>{t('action.cancel')}</Button>
            <Button
              variant="primary"
              disabled={acceptInvoice.isPending}
              onClick={() =>
                treatment.invoice &&
                acceptInvoice.mutate({
                  id: treatment.invoice.id,
                  data: {
                    recipient_emails: [...recipients, extraRecipient].filter(
                      (email) => email.trim() !== '',
                    ),
                  },
                })
              }
            >
              {t('invoices.accept')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          {treatment.customer_emails.length > 0 ? (
            <fieldset className="flex flex-col gap-1 border-0 p-0">
              <legend className="text-xs font-semibold text-ink-soft">
                {t('customers.emails')}
              </legend>
              {treatment.customer_emails.map((email) => (
                <CheckboxField
                  key={email}
                  label={email}
                  checked={recipients.includes(email)}
                  onChange={() => toggleRecipient(email)}
                />
              ))}
            </fieldset>
          ) : null}
          <TextField
            label={t('field.email')}
            type="email"
            inputMode="email"
            value={extraRecipient}
            onChange={(event) => setExtraRecipient(event.target.value)}
          />
        </div>
      </Dialog>

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
