import { Mail } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAddCustomerEmail, useGetInvoice, useGetTreatment } from '@/api/generated/endpoints'
import type { Invoice, InvoiceStatus, Treatment } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { CheckboxField, TextField } from '@/components/ui/field'
import { useRecipientLabel } from '@/features/treatments/useRecipientLabel'
import { useGoBack } from '@/lib/trail'

/** What the vet decided on the form: who the mail goes to. */
interface MailChoice {
  recipients: string[]
}

interface InvoiceMailFormProps {
  treatmentId: number
  title: string
  submitLabel: string
  /**
   * Whether the page applies to the invoice's state. Both pages are reachable by typing their
   * address, so they check it themselves rather than trusting the button that led there.
   */
  applies: (status: InvoiceStatus | undefined) => boolean
  /** Said instead of the form when it does not apply. */
  nothingToDo: string
  /** The addresses offered as checkboxes, and which of them start ticked. */
  offered: (
    record: Treatment,
    invoice: Invoice | undefined,
  ) => { addresses: string[]; ticked: string[] }
  /** The button needs an address — not when releasing, which may go without a mail. */
  requiresRecipient: boolean
  /** Shown above the buttons: what the action does beyond the mail. */
  notice?: ReactNode
  failedText: string
  pending: boolean
  failed: boolean
  submit: (invoiceId: number, choice: MailChoice) => void
}

/**
 * The page that decides where an invoice mail goes — releasing the invoice, or sending it
 * again. The two used to be two copies that drifted apart (the typed address trimmed on one,
 * offered to the customer's master data on one, an error shown on the other); what differs
 * between them now comes in as props.
 */
export function InvoiceMailForm({
  treatmentId,
  title,
  submitLabel,
  applies,
  nothingToDo,
  offered,
  requiresRecipient,
  notice,
  failedText,
  pending,
  failed,
  submit,
}: InvoiceMailFormProps) {
  const { t } = useTranslation()
  const treatment = useGetTreatment(treatmentId)
  const invoiceId = treatment.data?.invoice?.id
  const invoice = useGetInvoice(invoiceId ?? 0, { query: { enabled: Boolean(invoiceId) } })
  const recipientLabel = useRecipientLabel(treatment.data?.customer_id)
  const [chosen, setChosen] = useState<string[] | null>(null)
  const [extra, setExtra] = useState('')
  const [confirmSaveEmail, setConfirmSaveEmail] = useState(false)
  const addEmail = useAddCustomerEmail()
  // Back where the vet came from — the appointment with its animal's tab.
  const back = useGoBack({ to: '/treatments/$id', params: { id: String(treatmentId) } })

  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = treatment.data
  const current = record.invoice

  const header = (
    <PageHeader
      back={
        <BackLink
          to="/treatments/$id"
          params={{ id: String(treatmentId) }}
          label={record.patients.map((patient) => patient.name).join(', ') || t('treatments.title')}
        />
      }
      eyebrow={current ? <span className="numeric">{current.invoice_number}</span> : undefined}
      title={title}
    />
  )

  if (!current || !applies(current.status)) {
    return (
      <div className="mx-auto max-w-2xl">
        {header}
        <p className="mt-5 text-sm text-ink-soft">{nothingToDo}</p>
      </div>
    )
  }

  const { addresses, ticked } = offered(record, invoice.data)
  const recipients = chosen ?? ticked
  const toggle = (email: string) =>
    setChosen(
      recipients.includes(email)
        ? recipients.filter((entry) => entry !== email)
        : [...recipients, email],
    )

  const typedEmail = extra.trim()
  // An address typed here is usually one the customer has just given the practice, so it is
  // worth keeping — but that is a change to their master data, and it gets asked for
  // (issues.md 15).
  const isNewAddress = typedEmail !== '' && !record.customer_emails.includes(typedEmail)
  const everyone = [...new Set([...recipients, typedEmail])].filter((email) => email !== '')

  const go = () => {
    setConfirmSaveEmail(false)
    submit(current.id, { recipients: everyone })
  }

  const keepAddressAndGo = () => {
    const customerId = record.customer_id
    if (!customerId) return go()
    // Go ahead whatever storing the address does: a rejected address (the backend validates
    // the syntax) must not hold up the invoice it was typed for.
    addEmail.mutate({ id: customerId, data: { email: typedEmail } }, { onSettled: go })
  }

  return (
    <div className="mx-auto max-w-2xl">
      {header}

      <p className="mt-3 text-sm text-ink-soft">{t('invoices.recipients')}</p>

      <section className="mt-4 flex flex-col gap-3 rounded-card border border-line bg-surface p-4">
        {addresses.length > 0 ? (
          <fieldset className="flex flex-col gap-1 border-0 p-0">
            <legend className="text-xs font-semibold text-ink-soft">{t('customers.emails')}</legend>
            {addresses.map((email) => (
              <CheckboxField
                key={email}
                label={recipientLabel(email)}
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

      {notice}

      {/* The mail server is never asked from here (migration 0026): what can fail on this
          page is the request itself. */}
      {failed ? (
        <p
          className="mt-3 rounded-card border border-danger/30 bg-danger/5 px-3 py-2 text-sm text-danger"
          role="alert"
        >
          {failedText}
        </p>
      ) : null}

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={pending || (requiresRecipient && everyone.length === 0)}
          onClick={() => (isNewAddress ? setConfirmSaveEmail(true) : go())}
        >
          <Mail className="size-4" />
          {submitLabel}
        </Button>
      </div>

      <Dialog
        open={confirmSaveEmail}
        onOpenChange={setConfirmSaveEmail}
        title={t('invoices.saveEmailTitle')}
        footer={
          <>
            <Button onClick={go}>{t('invoices.saveEmailDecline')}</Button>
            <Button variant="primary" onClick={keepAddressAndGo}>
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
