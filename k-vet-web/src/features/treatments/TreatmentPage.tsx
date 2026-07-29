import { useQueryClient } from '@tanstack/react-query'
import { Link, useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import {
  getGetTreatmentQueryKey,
  useGetTreatment,
  useListTreatmentItems,
  usePatchTreatment,
} from '@/api/generated/endpoints'
import type { Treatment } from '@/api/generated/model'
import { PageHeader } from '@/components/PageHeader'
import { SaveIndicator } from '@/components/SaveIndicator'
import { TextAreaField } from '@/components/ui/field'
import { WarningBanner } from '@/components/WarningBanner'
import { InvoicePanel } from '@/features/treatments/InvoicePanel'
import { LineEditor } from '@/features/treatments/LineEditor'
import { PatientAttach } from '@/features/treatments/PatientAttach'
import { useAutoSave } from '@/lib/autosave'
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
  const client = useQueryClient()
  const { dateTime } = useLocaleFormat()

  const treatment = useGetTreatment(treatmentId)
  const items = useListTreatmentItems(treatmentId)
  const patchTreatment = usePatchTreatment()

  const autoSave = useAutoSave<Treatment>({
    save: (patch) =>
      patchTreatment.mutateAsync({
        id: treatmentId,
        data: patch as Parameters<typeof patchTreatment.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => client.setQueryData(getGetTreatmentQueryKey(treatmentId), updated),
  })

  if (treatment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>

  const record = treatment.data
  const warnings = record.patients.filter((patient) => patient.warning_remark)

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        eyebrow={
          <Link
            to="/appointments/$id"
            params={{ id: String(record.appointment_id) }}
            className="hover:underline"
          >
            {dateTime(record.starts_at) || t('appointments.title')}
          </Link>
        }
        title={record.patients.map((patient) => patient.name).join(', ') || t('treatments.title')}
        actions={<SaveIndicator state={autoSave.state} error={autoSave.error} />}
      />

      {warnings.map((patient) => (
        <WarningBanner
          key={patient.patient_id}
          className="mt-4"
          title={patient.name}
          remark={patient.warning_remark ?? ''}
        />
      ))}

      <section className="mt-5 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
        <TextAreaField
          label={t('field.treatmentReason')}
          defaultValue={record.treatment_reason ?? ''}
          disabled={record.frozen}
          onChange={(event) => autoSave.set({ treatment_reason: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <TextAreaField
          label={t('field.finding')}
          defaultValue={record.finding ?? ''}
          disabled={record.frozen}
          onChange={(event) => autoSave.set({ finding: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
      </section>

      <PatientAttach treatment={record} />

      <LineEditor treatment={record} items={items.data ?? []} readOnly={record.frozen} />

      <InvoicePanel treatment={record} hasItems={(items.data ?? []).length > 0} />
    </div>
  )
}
