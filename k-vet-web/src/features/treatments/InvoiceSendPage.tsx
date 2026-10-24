import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetTreatmentQueryKey,
  useSendInvoice,
} from '@/api/generated/endpoints'
import { InvoiceMailForm, useQueuedToast } from '@/features/treatments/InvoiceMailForm'
import { useGoBack } from '@/lib/trail'

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
  const client = useQueryClient()
  const announce = useQueuedToast()

  // Back where the vet came from — the appointment with its animal's tab — rather than piling
  // another entry onto the history.
  const back = useGoBack({ to: '/treatments/$id', params: { id: String(treatmentId) } })
  const sendInvoice = useSendInvoice({
    mutation: {
      onSuccess: async (queued) => {
        // Queued, not sent: the queue delivers it with its next run or at the time chosen, and
        // the appointment says so.
        announce(queued.email_queue)
        await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
        await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(queued.id) })
        await back()
      },
    },
  })

  return (
    <InvoiceMailForm
      treatmentId={treatmentId}
      title={t('invoices.sendTitle')}
      submitLabel={t('invoices.send')}
      applies={(status) => status === 'accepted' || status === 'sent' || status === 'submitted'}
      nothingToDo={t('invoices.nothingToSend')}
      // Where it went last time, or the customer's addresses when it has never gone anywhere.
      offered={(record, invoice) => {
        const stored = invoice?.email_recipients ?? []
        return {
          addresses: [...new Set([...record.customer_emails, ...stored])],
          ticked: stored.length > 0 ? stored : record.customer_emails,
        }
      }}
      requiresRecipient
      failedText={t('invoices.sendFailed')}
      pending={sendInvoice.isPending}
      failed={sendInvoice.isError}
      submit={(invoiceId, choice) =>
        sendInvoice.mutate({
          id: invoiceId,
          data: {
            recipient_emails: choice.recipients,
            send_at: choice.sendAt,
            template: choice.template,
          },
        })
      }
    />
  )
}
