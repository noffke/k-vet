import { useQueryClient } from '@tanstack/react-query'
import { Link, useParams } from '@tanstack/react-router'
import { Archive, ArchiveRestore, PawPrint, Upload } from 'lucide-react'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { apiFetch } from '@/api/fetcher'
import {
  getAttachmentThumbnailUrl,
  getGetPatientQueryKey,
  getListPatientFilesQueryKey,
  getListPatientsQueryKey,
  useArchivePatient,
  useGetPatient,
  useListPatientFiles,
  usePatchPatient,
  useUnarchivePatient,
} from '@/api/generated/endpoints'
import type { Attachment, Patient } from '@/api/generated/model'
import { DateInput } from '@/components/DateInput'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { CheckboxField, SelectField, TextAreaField, TextField } from '@/components/ui/field'
import { WarningBanner } from '@/components/WarningBanner'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'

const SEXES = ['female', 'male', 'unknown'] as const

/**
 * One animal: identity, medical identifiers, photo and the files the customer brought.
 * A recorded date of death archives the record — the practice keeps the history.
 */
export function PatientDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/patients/$id' })
  const patientId = Number(id)
  const client = useQueryClient()
  const { date } = useLocaleFormat()
  const photoInput = useRef<HTMLInputElement>(null)
  const fileInput = useRef<HTMLInputElement>(null)

  const patient = useGetPatient(patientId)
  const files = useListPatientFiles(patientId)
  const patchPatient = usePatchPatient()
  const archive = useArchivePatient()
  const unarchive = useUnarchivePatient()

  const store = (updated: Patient) => {
    client.setQueryData(getGetPatientQueryKey(patientId), updated)
    void client.invalidateQueries({ queryKey: getListPatientsQueryKey() })
  }

  const autoSave = useAutoSave<Patient>({
    save: (patch) =>
      patchPatient.mutateAsync({
        id: patientId,
        data: patch as Parameters<typeof patchPatient.mutateAsync>[0]['data'],
      }),
    onSaved: store,
  })

  /** Uploads a file and returns its attachment metadata. */
  const upload = async (file: File, extra: Record<string, string>) => {
    const form = new FormData()
    form.append('file', file)
    for (const [key, value] of Object.entries(extra)) form.append(key, value)
    return apiFetch<Attachment>('/api/attachments', { method: 'POST', body: form })
  }

  if (patient.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!patient.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = patient.data

  const field = (key: keyof Patient & string, label: string) => (
    <TextField
      label={label}
      defaultValue={(record[key] as string | null) ?? ''}
      error={autoSave.fieldErrors[key] ? t(autoSave.fieldErrors[key] ?? '') : undefined}
      onChange={(event) => autoSave.set({ [key]: event.target.value || null } as Partial<Patient>)}
      onBlur={() => void autoSave.flush()}
    />
  )

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        eyebrow={
          record.customer_id ? (
            <Link
              to="/customers/$id"
              params={{ id: String(record.customer_id) }}
              className="hover:underline"
            >
              {record.customer_name ?? t('customers.title')}
            </Link>
          ) : (
            t('patients.title')
          )
        }
        title={record.name ?? t('patients.new')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            {record.archived ? (
              <Button onClick={() => unarchive.mutate({ id: patientId }, { onSuccess: store })}>
                <ArchiveRestore className="size-4" />
                <span className="hidden sm:inline">{t('record.unarchive')}</span>
              </Button>
            ) : (
              <Button onClick={() => archive.mutate({ id: patientId }, { onSuccess: store })}>
                <Archive className="size-4" />
                <span className="hidden sm:inline">{t('record.archive')}</span>
              </Button>
            )}
          </>
        }
      >
        <span className="flex gap-1">
          <IncompleteBadge missing={record.missing_fields} />
          <ArchivedBadge archived={record.archived} />
        </span>
      </PageHeader>

      <WarningBanner className="mt-4" remark={record.warning_remark ?? ''} />

      {/* The photo sits on top, in a circle (FR-008). */}
      <div className="mt-5 flex flex-col items-center gap-2">
        <button
          type="button"
          onClick={() => photoInput.current?.click()}
          className="flex size-24 items-center justify-center overflow-hidden rounded-full border-2 border-line-strong bg-sunken"
          aria-label={t('patients.photo')}
        >
          {record.photo_attachment_id ? (
            <img
              src={getAttachmentThumbnailUrl(record.photo_attachment_id)}
              alt={record.name ?? t('patients.photo')}
              className="size-full object-cover"
            />
          ) : (
            <PawPrint className="size-8 text-ink-faint" />
          )}
        </button>
        <input
          ref={photoInput}
          type="file"
          accept="image/*"
          className="hidden"
          onChange={async (event) => {
            const file = event.target.files?.[0]
            if (!file) return
            const attachment = await upload(file, { kind: 'referenced' })
            autoSave.set({ photo_attachment_id: attachment.id })
            await autoSave.flush()
          }}
        />
        <span className="text-xs text-ink-faint">{t('patients.photo')}</span>
      </div>

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        {field('name', t('field.name'))}
        <SelectField
          label={t('field.sex')}
          defaultValue={record.sex ?? ''}
          onChange={(event) => autoSave.set({ sex: event.target.value || null })}
        >
          <option value="">—</option>
          {SEXES.map((sex) => (
            <option key={sex} value={sex}>
              {t(`sex.${sex}`)}
            </option>
          ))}
        </SelectField>

        {/* Cat and Dog are offered, anything else can be typed (FR-008). */}
        <TextField
          label={t('field.species')}
          list="species-options"
          defaultValue={record.species ?? ''}
          onChange={(event) => autoSave.set({ species: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <datalist id="species-options">
          <option value={t('species.cat')} />
          <option value={t('species.dog')} />
        </datalist>
        {field('race', t('field.race'))}
        {field('colour', t('field.colour'))}
        {/* One decimal: NumberInput emits toFixed(decimals), so a second one never reaches the
            wire — the column is NUMERIC(5,1) and would otherwise round it away silently. */}
        <NumberInput
          label={t('field.weight')}
          value={record.weight_kg ?? null}
          decimals={1}
          onChange={(value) => {
            autoSave.set({ weight_kg: value })
            void autoSave.flush()
          }}
        />
        <DateInput
          label={t('field.dateOfBirth')}
          value={record.date_of_birth ?? null}
          onChange={(value) => {
            autoSave.set({ date_of_birth: value })
            void autoSave.flush()
          }}
        />
        {field('chip_number', t('field.chipNumber'))}
        {field('eu_passport_number', t('field.euPassportNumber'))}
        <DateInput
          label={t('field.dateOfDeath')}
          value={record.date_of_death ?? null}
          hint={record.date_of_death ? t('patients.deceasedArchived') : undefined}
          onChange={(value) => {
            autoSave.set({ date_of_death: value })
            void autoSave.flush()
          }}
        />
        <div className="flex flex-wrap items-center gap-4">
          <CheckboxField
            label={t('field.neutered')}
            defaultChecked={record.neutered}
            onChange={(event) => {
              autoSave.set({ neutered: event.target.checked })
              void autoSave.flush()
            }}
          />
          <CheckboxField
            label={t('field.insured')}
            defaultChecked={record.insured}
            onChange={(event) => {
              autoSave.set({ insured: event.target.checked })
              void autoSave.flush()
            }}
          />
        </div>
        <TextAreaField
          label={t('field.warningRemark')}
          defaultValue={record.warning_remark ?? ''}
          wrapperClassName="sm:col-span-2"
          onChange={(event) => autoSave.set({ warning_remark: event.target.value || null })}
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
          <input
            ref={fileInput}
            type="file"
            className="hidden"
            onChange={async (event) => {
              const file = event.target.files?.[0]
              if (!file) return
              await upload(file, { kind: 'patient_file', patient_id: String(patientId) })
              await client.invalidateQueries({
                queryKey: getListPatientFilesQueryKey(patientId),
              })
            }}
          />
        </div>

        <ul className="mt-2 flex flex-col gap-2">
          {(files.data ?? []).map((file) => (
            <li
              key={file.id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-line bg-surface px-3 py-2"
            >
              <span className="min-w-0 truncate">{file.orig_name}</span>
              <span className="text-xs text-ink-faint">
                {file.reference_date ? date(file.reference_date) : date(file.created_at)}
                {file.note ? ` · ${file.note}` : ''}
              </span>
            </li>
          ))}
          {(files.data ?? []).length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>
    </div>
  )
}
