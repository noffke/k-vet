import { useQueryClient } from '@tanstack/react-query'
import { Link, useParams } from '@tanstack/react-router'
import { Upload } from 'lucide-react'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { apiFetch } from '@/api/fetcher'
import {
  getGetPatientTreatmentQueryKey,
  getGetTreatmentQueryKey,
  getListPatientTreatmentFilesQueryKey,
  useGetPatientTreatment,
  useGetTreatment,
  useListPatientTreatmentFiles,
  useListTreatmentItems,
  usePatchPatientTreatment,
} from '@/api/generated/endpoints'
import type { Attachment, PatientTreatment } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { TextAreaField } from '@/components/ui/field'
import { PositionGroup } from '@/features/treatments/LineEditor'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'

/**
 * One animal's record of one visit: why it was seen, what was found, the files that belong to
 * it, and the positions billed for it.
 *
 * The positions are here as well as on the treatment because this is where the vet stands
 * during the examination — and they belong to this record, which is what ties a dispensed
 * batch to the animal that received it.
 */
export function PatientTreatmentPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/patient-treatments/$id' })
  const recordId = Number(id)
  const client = useQueryClient()
  const { date, dateTime } = useLocaleFormat()
  const fileInput = useRef<HTMLInputElement>(null)

  const record = useGetPatientTreatment(recordId)
  const files = useListPatientTreatmentFiles(recordId)
  const treatmentId = record.data?.treatment_id
  const treatment = useGetTreatment(treatmentId ?? 0, {
    query: { enabled: Boolean(treatmentId) },
  })
  const items = useListTreatmentItems(treatmentId ?? 0, {
    query: { enabled: Boolean(treatmentId) },
  })
  const patchRecord = usePatchPatientTreatment()

  const autoSave = useAutoSave<PatientTreatment>({
    save: (patch) =>
      patchRecord.mutateAsync({
        id: recordId,
        data: patch as Parameters<typeof patchRecord.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => {
      client.setQueryData(getGetPatientTreatmentQueryKey(recordId), updated)
      if (treatmentId) {
        void client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
      }
    },
  })

  if (record.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!record.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const current = record.data

  const upload = async (file: File) => {
    const form = new FormData()
    form.append('file', file)
    form.append('kind', 'treatment_file')
    form.append('patient_treatment_id', String(recordId))
    await apiFetch<Attachment>('/api/attachments', { method: 'POST', body: form })
    await client.invalidateQueries({
      queryKey: getListPatientTreatmentFilesQueryKey(recordId),
    })
  }

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={
          <BackLink
            to="/treatments/$id"
            params={{ id: String(current.treatment_id) }}
            label={dateTime(current.starts_at) || t('treatments.title')}
          />
        }
        eyebrow={
          <Link
            to="/patients/$id"
            params={{ id: String(current.patient_id) }}
            className="hover:underline"
          >
            {t('patients.title')}
          </Link>
        }
        title={current.patient_name ?? t('field.patient')}
        actions={<SaveIndicator state={autoSave.state} error={autoSave.error} />}
      />

      <section className="mt-5 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
        <TextAreaField
          label={t('field.treatmentReason')}
          defaultValue={current.treatment_reason ?? ''}
          onChange={(event) => autoSave.set({ treatment_reason: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <TextAreaField
          label={t('field.finding')}
          defaultValue={current.finding ?? ''}
          onChange={(event) => autoSave.set({ finding: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
      </section>

      <section className="mt-6">
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-base">{t('patients.files')}</h2>
          <Button size="small" onClick={() => fileInput.current?.click()}>
            <Upload className="size-4" />
            {t('action.upload')}
          </Button>
        </div>
        <input
          ref={fileInput}
          type="file"
          className="hidden"
          onChange={async (event) => {
            const file = event.target.files?.[0]
            if (file) await upload(file)
          }}
        />
        <ul className="mt-2 flex flex-col gap-2">
          {(files.data ?? []).map((file) => (
            <li
              key={file.id}
              className="flex items-center justify-between gap-3 rounded-card border border-line bg-surface px-3 py-2 text-sm"
            >
              <a href={`/api/attachments/${file.id}`} className="text-rust hover:underline">
                {file.orig_name}
              </a>
              <span className="numeric text-xs text-ink-faint">{date(file.created_at)}</span>
            </li>
          ))}
          {(files.data ?? []).length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-4 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>

      {treatment.data ? (
        <section className="mt-6">
          <h2 className="text-base">{t('treatments.items')}</h2>
          <PositionGroup
            treatment={treatment.data}
            heading={current.patient_name ?? t('field.patient')}
            patientTreatmentId={recordId}
            items={(items.data ?? []).filter((item) => item.patient_treatment_id === recordId)}
            readOnly={current.frozen}
          />
        </section>
      ) : null}
    </div>
  )
}
