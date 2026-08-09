import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { Mail } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  useGetInvoice,
  useGetTreatment,
  useSendInvoice,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { CheckboxField, TextField } from '@/components/ui/field'

/**
 * Sending a released invoice by e-mail, again or for the first time.
 *
 * The addresses are chosen here rather than reused silently, because the two reasons to be on
 * this page are that the last send failed and that it went to the wrong place.
 */
export function InvoiceSendPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id/invoice/send' })
  const treatmentId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()

  const treatment = useGetTreatment(treatmentId)
  const invoiceId = treatment.data?.invoice?.id
  const invoice = useGetInvoice(invoiceId ?? 0, { query: { enabled: Boolean(invoiceId) } })
  const [chosen, setChosen] = useState<string[] | null>(null)
  const [extra, setExtra] = useState('')

  const back = () => navigate({ to: '/treatments/$id', params: { id: String(treatmentId) } })
  const sendInvoice = useSendInvoice({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
        if (invoiceId) {
          await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(invoiceId) })
        }
        await back()
      },
    },
  })

  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = treatment.data
  const status = record.invoice?.status

  const header = (
    <PageHeader
      back={
        <BackLink
          to="/treatments/$id"
          params={{ id: String(treatmentId) }}
          label={record.patients.map((patient) => patient.name).join(', ') || t('treatments.title')}
        />
      }
      eyebrow={
        record.invoice ? (
          <span className="numeric">{record.invoice.invoice_number}</span>
        ) : undefined
      }
      title={t('invoices.sendTitle')}
    />
  )

  // The page is reachable by typing its address, so it checks the invoice's state itself
  // rather than trusting the button that led here.
  if (!status || status === 'created' || status === 'cancelled') {
    return (
      <div className="mx-auto max-w-2xl">
        {header}
        <p className="mt-5 text-sm text-ink-soft">{t('invoices.nothingToAccept')}</p>
      </div>
    )
  }

  // Where it went last time, or the customer's addresses when it has never gone anywhere.
  const stored = invoice.data?.email_recipients ?? []
  const known = [...new Set([...record.customer_emails, ...stored])]
  const recipients = chosen ?? (stored.length > 0 ? stored : record.customer_emails)
  const toggle = (email: string) =>
    setChosen(
      recipients.includes(email)
        ? recipients.filter((entry) => entry !== email)
        : [...recipients, email],
    )
  const addresses = [...recipients, extra].filter((email) => email.trim() !== '')

  return (
    <div className="mx-auto max-w-2xl">
      {header}

      <p className="mt-3 text-sm text-ink-soft">{t('invoices.recipients')}</p>

      <section className="mt-4 flex flex-col gap-3 rounded-card border border-line bg-surface p-4">
        {known.length > 0 ? (
          <fieldset className="flex flex-col gap-1 border-0 p-0">
            <legend className="text-xs font-semibold text-ink-soft">{t('customers.emails')}</legend>
            {known.map((email) => (
              <CheckboxField
                key={email}
                label={email}
                checked={recipients.includes(email)}
                onChange={() => toggle(email)}
              />
            ))}
          </fieldset>
        ) : null}
        <TextField
          label={t('field.email')}
          type="email"
          inputMode="email"
          value={extra}
          onChange={(event) => setExtra(event.target.value)}
        />
      </section>

      {sendInvoice.isError ? (
        <p className="mt-3 text-sm text-danger" role="alert">
          {t('save.failed')}
        </p>
      ) : null}

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={addresses.length === 0 || sendInvoice.isPending}
          onClick={() =>
            record.invoice &&
            sendInvoice.mutate({
              id: record.invoice.id,
              data: { recipient_emails: addresses },
            })
          }
        >
          <Mail className="size-4" />
          {t('invoices.send')}
        </Button>
      </div>
    </div>
  )
}
