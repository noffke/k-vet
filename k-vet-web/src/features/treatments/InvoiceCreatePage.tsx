import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { ReceiptText } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  getInvoicePdfUrl,
  useCreateInvoice,
  useGetInvoice,
  useGetTreatment,
  useListTreatmentItems,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { CheckboxField, TextAreaField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Creating the invoice for a treatment, or making it again after the lines changed.
 *
 * Updating an accepted invoice cancels it and burns its number, so that warning belongs on
 * the screen where the decision is taken.
 */
export function InvoiceCreatePage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id/invoice' })
  const treatmentId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()
  const { money } = useLocaleFormat()

  const treatment = useGetTreatment(treatmentId)
  const items = useListTreatmentItems(treatmentId)
  const invoiceId = treatment.data?.invoice?.id
  const invoice = useGetInvoice(invoiceId ?? 0, { query: { enabled: Boolean(invoiceId) } })

  const [includesFinding, setIncludesFinding] = useState<boolean | null>(null)
  const [note, setNote] = useState<string | null>(null)
  // Opened empty on the click and pointed at the PDF when it exists: a tab opened from an
  // awaited mutation is a popup, and every browser blocks that.
  const pdfTab = useRef<Window | null>(null)

  const back = () => navigate({ to: '/treatments/$id', params: { id: String(treatmentId) } })
  const createInvoice = useCreateInvoice({
    mutation: {
      onSuccess: async (created) => {
        if (pdfTab.current && !pdfTab.current.closed) {
          pdfTab.current.location.href = getInvoicePdfUrl(created.id)
        }
        pdfTab.current = null
        await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
        if (invoiceId) {
          await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(invoiceId) })
        }
        await back()
      },
      onError: () => {
        pdfTab.current?.close()
        pdfTab.current = null
      },
    },
  })

  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = treatment.data
  const status = record.invoice?.status
  const isAccepted = status === 'accepted' || status === 'submitted'
  const live = record.invoice && status !== 'cancelled' ? record.invoice : null
  const hasItems = (items.data ?? []).length > 0

  // A new invoice lists the finding; an existing one keeps what it was created with.
  const finding = includesFinding ?? invoice.data?.includes_finding ?? true
  const noteText = note ?? invoice.data?.note ?? ''

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        back={
          <BackLink
            to="/treatments/$id"
            params={{ id: String(treatmentId) }}
            label={
              record.patients.map((patient) => patient.name).join(', ') || t('treatments.title')
            }
          />
        }
        title={live ? t('invoices.update') : t('invoices.create')}
      />

      {isAccepted ? (
        <p className="mt-3 rounded-card border border-rust/30 bg-rust-soft/40 px-3 py-2 text-sm text-rust">
          {t('invoices.acceptedWillCancel')}
        </p>
      ) : null}

      <section className="mt-5 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
        <CheckboxField
          label={t('invoices.includesFinding')}
          checked={finding}
          onChange={(event) => setIncludesFinding(event.target.checked)}
        />
        <TextAreaField
          label={t('field.note')}
          value={noteText}
          onChange={(event) => setNote(event.target.value)}
        />
        <p className="numeric text-sm text-ink-soft">
          {t('treatments.total')}: {money(record.total_gross)}
        </p>
      </section>

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={!hasItems || createInvoice.isPending}
          onClick={() => {
            pdfTab.current = window.open('', '_blank')
            createInvoice.mutate({
              id: treatmentId,
              data: { includes_finding: finding, note: noteText || null },
            })
          }}
        >
          <ReceiptText className="size-4" />
          {live ? t('invoices.update') : t('invoices.create')}
        </Button>
      </div>
    </div>
  )
}
