import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { Mail } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  useAcceptInvoice,
  useAddCustomerEmail,
  useGetTreatment,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { CheckboxField, TextField } from '@/components/ui/field'

/**
 * Releasing the invoice: it gets its timestamp, the treatment freezes, and the document goes
 * to the addresses ticked here. Everything on this page is about who receives it.
 */
export function InvoiceAcceptPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id/invoice/accept' })
  const treatmentId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()

  const treatment = useGetTreatment(treatmentId)
  const [chosen, setChosen] = useState<string[] | null>(null)
  const [extra, setExtra] = useState('')
  const [confirmSaveEmail, setConfirmSaveEmail] = useState(false)
  const addEmail = useAddCustomerEmail()

  const back = () => navigate({ to: '/treatments/$id', params: { id: String(treatmentId) } })
  const acceptInvoice = useAcceptInvoice({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
        const invoiceId = treatment.data?.invoice?.id
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
  const invoice = record.invoice

  // The customer's addresses start ticked; the page is reachable by typing its address, so it
  // checks the invoice's state itself rather than trusting the button that led here.
  const recipients = chosen ?? record.customer_emails
  const toggle = (email: string) =>
    setChosen(
      recipients.includes(email)
        ? recipients.filter((entry) => entry !== email)
        : [...recipients, email],
    )

  if (invoice?.status !== 'created') {
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
          title={t('invoices.accept')}
        />
        <p className="mt-5 text-sm text-ink-soft">{t('invoices.nothingToAccept')}</p>
      </div>
    )
  }

  const typedEmail = extra.trim()
  // An address typed here is usually one the customer has just given the practice, so it is
  // worth keeping — but that is a change to their master data, and it gets asked for
  // (issues.md 15).
  const isNewAddress = typedEmail !== '' && !record.customer_emails.includes(typedEmail)

  const accept = () => {
    setConfirmSaveEmail(false)
    acceptInvoice.mutate({
      id: invoice.id,
      data: {
        recipient_emails: [...recipients, typedEmail].filter((email) => email !== ''),
      },
    })
  }

  const keepAddressAndAccept = () => {
    const customerId = record.customer_id
    if (!customerId) return accept()
    // Accept regardless of what storing the address does: a rejected address (the backend
    // validates the syntax) must not hold up releasing the invoice it was typed for.
    addEmail.mutate({ id: customerId, data: { email: typedEmail } }, { onSettled: accept })
  }

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
        eyebrow={<span className="numeric">{invoice.invoice_number}</span>}
        title={t('invoices.accept')}
      />

      <p className="mt-3 text-sm text-ink-soft">{t('invoices.recipients')}</p>

      <section className="mt-4 flex flex-col gap-3 rounded-card border border-line bg-surface p-4">
        {record.customer_emails.length > 0 ? (
          <fieldset className="flex flex-col gap-1 border-0 p-0">
            <legend className="text-xs font-semibold text-ink-soft">{t('customers.emails')}</legend>
            {record.customer_emails.map((email) => (
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
          label={t('invoices.additionalEmail')}
          type="email"
          inputMode="email"
          value={extra}
          onChange={(event) => setExtra(event.target.value)}
        />
      </section>

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={acceptInvoice.isPending}
          onClick={() => (isNewAddress ? setConfirmSaveEmail(true) : accept())}
        >
          <Mail className="size-4" />
          {t('invoices.accept')}
        </Button>
      </div>

      <Dialog
        open={confirmSaveEmail}
        onOpenChange={setConfirmSaveEmail}
        title={t('invoices.saveEmailTitle')}
        footer={
          <>
            <Button onClick={accept}>{t('invoices.saveEmailDecline')}</Button>
            <Button variant="primary" onClick={keepAddressAndAccept}>
              {t('invoices.saveEmailConfirm')}
            </Button>
          </>
        }
      >
        <p className="text-sm text-ink-soft">
          {t('invoices.saveEmailBody', { email: typedEmail })}
        </p>
      </Dialog>
    </div>
  )
}
