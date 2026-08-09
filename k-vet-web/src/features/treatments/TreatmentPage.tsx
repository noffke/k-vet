import { Link, useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useGetTreatment, useListTreatmentItems } from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { NotBilledBadge } from '@/components/RecordBadges'
import { WarningBanner } from '@/components/WarningBanner'
import { InvoicePanel } from '@/features/treatments/InvoicePanel'
import { LineEditor } from '@/features/treatments/LineEditor'
import { PatientAttach } from '@/features/treatments/PatientAttach'
import { useLocaleFormat } from '@/lib/locale'

/**
 * The treatment record: what was done, on which animal, and what it costs. Reason and
 * finding auto-save while typing; the invoice panel sits at the bottom because that is
 * the last step of the visit.
 */
export function TreatmentPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/treatments/$id' })
  const treatmentId = Number(id)
  const { dateTime } = useLocaleFormat()

  const treatment = useGetTreatment(treatmentId)
  const items = useListTreatmentItems(treatmentId)

  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>

  const record = treatment.data
  const warnings = record.patients.filter((patient) => patient.warning_remark)

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={
          <BackLink
            to="/appointments/$id"
            params={{ id: String(record.appointment_id) }}
            label={dateTime(record.starts_at) || t('appointments.title')}
          />
        }
        title={record.patients.map((patient) => patient.name).join(', ') || t('treatments.title')}
        actions={<span className="text-sm text-ink-faint">{t('save.hint')}</span>}
      >
        {/* The same marker the appointment list carries, so following it lands somewhere
            that still says why (FR-036). */}
        <NotBilledBadge
          count={
            record.item_count > 0 && (!record.invoice || record.invoice.status === 'cancelled')
              ? 1
              : 0
          }
        />
      </PageHeader>

      {warnings.map((patient) => (
        <WarningBanner
          key={patient.patient_id}
          className="mt-4"
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
    </div>
  )
}
