import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useGetTreatment, useListTreatmentItems } from '@/api/generated/endpoints'
import { WarningBanner } from '@/components/WarningBanner'
import { InvoicePanel } from '@/features/treatments/InvoicePanel'
import { LineEditor } from '@/features/treatments/LineEditor'
import { PatientAttach } from '@/features/treatments/PatientAttach'

/**
 * The treatment of an appointment: what was done, on which animal, and what it costs.
 *
 * Every appointment has exactly one (a second invoice for the same visit is a second
 * appointment), so this is part of the appointment's page rather than a page of its own. The
 * invoice panel sits at the bottom because that is the last step of the visit.
 */
export function TreatmentSection({ treatmentId }: { treatmentId: number }) {
  const { t } = useTranslation()
  const treatment = useGetTreatment(treatmentId)
  const items = useListTreatmentItems(treatmentId)

  if (treatment.isPending) return <p className="mt-6 text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="mt-6 text-sm text-danger">{t('error.notFound')}</p>

  const record = treatment.data
  const warnings = record.patients.filter((patient) => patient.warning_remark)

  return (
    <section className="mt-6">
      {warnings.map((patient) => (
        <WarningBanner
          key={patient.patient_id}
          className="mb-4"
          title={patient.name}
          remark={patient.warning_remark ?? ''}
        />
      ))}

      <PatientAttach treatment={record} />

      {/* Why each animal was seen and what was found is its own record — two animals brought
          in together are rarely brought in for the same thing. */}
      <ul className="mt-3 flex flex-col gap-2">
        {record.patients.map((patient) => (
          <li key={patient.id}>
            <Link
              to="/patient-treatments/$id"
              params={{ id: String(patient.id) }}
              className="block rounded-card border border-line bg-surface px-3 py-2.5 hover:bg-cream-soft"
            >
              <span className="font-medium text-ink">{patient.name}</span>
              <span className="ml-2 text-sm text-ink-soft">
                {patient.treatment_reason ?? t('treatments.noReason')}
              </span>
            </Link>
          </li>
        ))}
      </ul>

      <LineEditor treatment={record} items={items.data ?? []} readOnly={record.frozen} />

      <InvoicePanel treatment={record} hasItems={(items.data ?? []).length > 0} />
    </section>
  )
}
