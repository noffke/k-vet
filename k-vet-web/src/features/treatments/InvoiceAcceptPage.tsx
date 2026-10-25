import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  getGetInvoiceQueryKey,
  getGetPatientQueryKey,
  getGetTreatmentQueryKey,
  getListPatientsQueryKey,
  useAcceptInvoice,
  useGetTreatment,
  useListTreatmentItems,
} from '@/api/generated/endpoints'
import { InvoiceMailForm, useQueuedToast } from '@/features/treatments/InvoiceMailForm'
import { linesWithoutKm } from '@/features/treatments/travelKm'
import { useLocaleFormat } from '@/lib/locale'
import { useGoBack } from '@/lib/trail'

/**
 * Releasing the invoice: it gets its timestamp, the treatment freezes, and the document goes
 * to the addresses ticked here. Everything on this page is about who receives it.
 */
export function InvoiceAcceptPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id/invoice/accept' })
  const treatmentId = Number(id)
  const client = useQueryClient()
  const announce = useQueuedToast()
  const treatment = useGetTreatment(treatmentId)
  // A Wegegeld line may have come after the invoice was created; the release waits for its km.
  const withoutKm = linesWithoutKm(useListTreatmentItems(treatmentId).data)

  // Back where the vet came from — the appointment with its animal's tab — rather than piling
  // another entry onto the history.
  const back = useGoBack({ to: '/treatments/$id', params: { id: String(treatmentId) } })
  const acceptInvoice = useAcceptInvoice({
    mutation: {
      onSuccess: async (released) => {
        // Released now; the mail goes out with the queue's next run, or at the time chosen.
        announce(released.email_queue)
        await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
        // A euthanasia line has just ended an animal's record: its page and every list of
        // animals (the next visit's picker among them) must not show it alive.
        await client.invalidateQueries({ queryKey: getListPatientsQueryKey() })
        for (const patient of treatment.data?.patients ?? []) {
          await client.invalidateQueries({ queryKey: getGetPatientQueryKey(patient.patient_id) })
        }
        await client.invalidateQueries({ queryKey: getGetInvoiceQueryKey(released.id) })
        await back()
      },
    },
  })

  return (
    <InvoiceMailForm
      treatmentId={treatmentId}
      title={t('invoices.accept')}
      submitLabel={t('invoices.accept')}
      applies={(status) => status === 'created'}
      nothingToDo={t('invoices.nothingToAccept')}
      // The customer's addresses, all ticked: a first release goes where the customer said.
      offered={(record) => ({ addresses: record.customer_emails, ticked: record.customer_emails })}
      requiresRecipient={false}
      notice={<EuthanasiaNotice treatmentId={treatmentId} />}
      blocked={
        withoutKm.length > 0
          ? t('invoices.travelWithoutKm', { names: withoutKm.map((item) => item.name).join(', ') })
          : null
      }
      failedText={t('invoices.acceptFailed')}
      pending={acceptInvoice.isPending}
      failed={acceptInvoice.isError}
      submit={(invoiceId, choice) =>
        acceptInvoice.mutate({
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

/**
 * Releasing sets the date of death of every animal a euthanasia drug was dispensed for — a
 * consequence on the animal's record, and one an archived, deceased animal cannot simply be
 * taken back from, so it is said before the button rather than discovered after it.
 */
function EuthanasiaNotice({ treatmentId }: { treatmentId: number }) {
  const { t } = useTranslation()
  const { date } = useLocaleFormat()
  const treatment = useGetTreatment(treatmentId)
  const items = useListTreatmentItems(treatmentId)
  const record = treatment.data
  if (!record) return null
  const putDown = record.patients.filter((patient) =>
    (items.data ?? []).some((item) => item.euthanasia && item.patient_treatment_id === patient.id),
  )
  if (putDown.length === 0) return null

  return (
    <aside
      role="note"
      className="mt-4 flex items-start gap-3 rounded-card border-2 border-danger/40 bg-blush/40 px-4 py-3"
    >
      <AlertTriangle className="mt-0.5 size-5 shrink-0 text-danger" aria-hidden />
      <p className="text-sm font-medium text-ink">
        {t('invoices.euthanasiaNotice', {
          names: putDown.map((patient) => patient.name).join(', '),
          date: date(record.starts_at),
        })}
      </p>
    </aside>
  )
}
