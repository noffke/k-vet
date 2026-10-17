import { Navigate, useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useGetTreatment } from '@/api/generated/endpoints'

/**
 * Where `/treatments/$id` leads now: the appointment it belongs to.
 *
 * A treatment is the other half of its appointment and is edited on the appointment's page.
 * The route stays so that every link carrying only a treatment id — dashboard, invoices, lots,
 * the invoice steps — and every bookmark still arrives; `replace` keeps the hop out of the
 * history, so the back button does not bounce off it.
 */
export function TreatmentPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id' })
  const treatment = useGetTreatment(Number(id))

  if (treatment.data) {
    return (
      <Navigate
        to="/appointments/$id"
        params={{ id: String(treatment.data.appointment_id) }}
        replace
      />
    )
  }
  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  return <p className="text-sm text-danger">{t('error.notFound')}</p>
}
