import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { Copy, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  getGetAppointmentQueryKey,
  getListAppointmentsQueryKey,
  getListTreatmentsQueryKey,
  useCreateTreatment,
  useDeleteAppointment,
  useDuplicateAppointment,
  useGetAppointment,
  useListTreatments,
  usePatchAppointment,
} from '@/api/generated/endpoints'
import type { Appointment, PriceMode } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { CustomerPicker } from '@/components/CustomerPicker'
import { DateInput } from '@/components/DateInput'
import { PageHeader } from '@/components/PageHeader'
import { IncompleteBadge, NotBilledBadge, NotSentBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { TimeInput } from '@/components/TimeInput'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { TextAreaField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'
import { todayIso } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'

/**
 * One appointment: date and time (auto-saved), a note, and its treatments.
 *
 * The date defaults to today and the time is always typed by the vet — the appointment
 * stays "incomplete" until it has one (FR-025).
 */
export function AppointmentDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/appointments/$id' })
  const appointmentId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()
  const { date: formatDate } = useLocaleFormat()

  const appointment = useGetAppointment(appointmentId)
  const treatments = useListTreatments(appointmentId)
  const patchAppointment = usePatchAppointment()
  const [duplicateOpen, setDuplicateOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  // A typed date has nowhere to go until a time exists — a timestamp needs both halves.
  const [typedDate, setTypedDate] = useState<string | null>(null)

  const autoSave = useAutoSave<Appointment>({
    save: (patch) =>
      patchAppointment.mutateAsync({
        id: appointmentId,
        data: patch as Parameters<typeof patchAppointment.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => {
      client.setQueryData(getGetAppointmentQueryKey(appointmentId), updated)
      void client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
    },
  })

  const createTreatment = useCreateTreatment({
    mutation: {
      onSuccess: async (treatment) => {
        await client.invalidateQueries({
          queryKey: getListTreatmentsQueryKey(appointmentId),
        })
        await navigate({ to: '/treatments/$id', params: { id: String(treatment.id) } })
      },
    },
  })

  const duplicate = useDuplicateAppointment({
    mutation: {
      onSuccess: async (copy) => {
        await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
        setDuplicateOpen(false)
        await navigate({ to: '/appointments/$id', params: { id: String(copy.id) } })
      },
    },
  })

  const removeAppointment = useDeleteAppointment({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
        await navigate({ to: '/appointments' })
      },
    },
  })

  if (appointment.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!appointment.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>

  const record = appointment.data
  const startsAt = record.starts_at ? new Date(record.starts_at) : null
  const storedDate = startsAt ? localIsoDate(startsAt) : null
  const isoTime = startsAt ? localTime(startsAt) : ''
  // Every Termin has a date; a new one opens on today and the vet corrects it (FR-025).
  const isoDate = typedDate ?? storedDate ?? todayIso()

  /** Combines the typed date and time into the ISO timestamp the API stores. */
  const writeStartsAt = (nextDate: string | null, nextTime: string) => {
    // Clearing the date field does not clear the day: fall back to what is stored.
    const day = nextDate ?? storedDate ?? todayIso()
    setTypedDate(day)
    if (!nextTime) {
      // Without a time the appointment stays incomplete — do not invent one. The date
      // waits in `typedDate` until the time arrives instead of being thrown away.
      if (record.starts_at) autoSave.set({ starts_at: null })
      return
    }
    const [hours = '0', minutes = '0'] = nextTime.split(':')
    const combined = new Date(`${day}T00:00:00`)
    combined.setHours(Number(hours), Number(minutes), 0, 0)
    autoSave.set({ starts_at: combined.toISOString() })
  }

  // The generated hook types the error as `void`; the fetcher throws `ApiError`.
  //
  // Every failure is shown, not only the one that was anticipated. Rendering the 409 alone is
  // how deleting came to look broken: anything else left the button doing nothing, silently.
  const deleteError: unknown = removeAppointment.error
  const deleteMessage =
    deleteError instanceof ApiError && deleteError.status === 409
      ? t('appointments.deleteBlocked')
      : deleteError
        ? t('error.generic')
        : null

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/appointments" label={t('appointments.title')} />}
        title={storedDate ? formatDate(storedDate) : t('appointments.new')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            <Button onClick={() => setDuplicateOpen(true)}>
              <Copy className="size-4" />
              <span className="sr-only sm:not-sr-only">{t('action.duplicate')}</span>
            </Button>
            <Button
              variant="danger"
              onClick={() => setDeleteOpen(true)}
              disabled={removeAppointment.isPending}
            >
              <Trash2 className="size-4" />
              <span className="sr-only sm:not-sr-only">{t('action.delete')}</span>
            </Button>
            <ConfirmDialog
              open={deleteOpen}
              onOpenChange={setDeleteOpen}
              title={t('appointments.delete')}
              confirmLabel={t('action.delete')}
              busy={removeAppointment.isPending}
              onConfirm={() => removeAppointment.mutate({ id: appointmentId })}
            >
              {t('appointments.deleteConfirm')}
            </ConfirmDialog>
          </>
        }
      >
        <IncompleteBadge missing={record.missing_fields} />
      </PageHeader>

      {deleteMessage ? (
        <p className="mt-3 rounded-card border border-danger/30 bg-danger/5 px-3 py-2 text-sm text-danger">
          {deleteMessage}
        </p>
      ) : null}

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <DateInput
          label={t('field.date')}
          value={isoDate}
          onChange={(next) => writeStartsAt(next, isoTime)}
          error={autoSave.fieldErrors.starts_at ? t('field.required') : undefined}
        />
        <TimeInput
          label={t('field.time')}
          value={isoTime}
          onChange={(next) => writeStartsAt(isoDate, next)}
          onBlur={() => void autoSave.flush()}
        />
        <TextAreaField
          label={t('field.note')}
          defaultValue={record.note ?? ''}
          wrapperClassName="sm:col-span-2"
          onChange={(event) => autoSave.set({ note: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />

        <div className="sm:col-span-2">
          <p className="eyebrow">{t('appointments.customer')}</p>
          <p className="mt-0.5 mb-2 text-xs text-ink-faint">{t('appointments.customerHint')}</p>
          {/* Locked once a treatment hangs off it: its animals belong to this customer, and
              moving the appointment would strand them. */}
          <CustomerPicker
            customerId={record.customer_id ?? null}
            customerName={record.customer_name ?? null}
            locked={(treatments.data ?? []).length > 0}
            onPick={(customerId) => {
              autoSave.set({ customer_id: customerId })
              void autoSave.flush()
            }}
            onClear={() => {
              autoSave.set({ customer_id: null })
              void autoSave.flush()
            }}
          />
        </div>
      </section>

      <section className="mt-6">
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-base">{t('appointments.treatments')}</h2>
          <Button
            variant="accent"
            size="small"
            // Without a customer there is nothing to limit the animals to, which is the whole
            // point of asking (issues.md 7); the server refuses it too.
            disabled={record.draft || !record.customer_id || createTreatment.isPending}
            title={record.draft ? t('record.incomplete') : undefined}
            onClick={() => createTreatment.mutate({ id: appointmentId, data: { patient_ids: [] } })}
          >
            <Plus className="size-4" />
            {t('appointments.addTreatment')}
          </Button>
        </div>

        {record.draft ? (
          <p className="mt-2 text-xs text-ink-faint">
            {t('record.incompleteWithFields', {
              fields: t('field.time'),
            })}
          </p>
        ) : null}
        {!record.draft && !record.customer_id ? (
          <p className="mt-2 text-xs text-ink-faint">{t('treatment.customerRequired')}</p>
        ) : null}

        <ul className="mt-3 flex flex-col gap-2">
          {(treatments.data ?? []).map((treatment) => (
            <li key={treatment.id}>
              <button
                type="button"
                onClick={() =>
                  void navigate({ to: '/treatments/$id', params: { id: String(treatment.id) } })
                }
                className="w-full rounded-card border border-line bg-surface px-3 py-3 text-left hover:bg-cream-soft"
              >
                <span className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium text-ink">
                    {treatment.patients.map((patient) => patient.name).join(', ') ||
                      t('field.patient')}
                  </span>
                  {/* Where the invoice number stands once there is one, and where its
                      absence used to be silence. */}
                  <span className="numeric flex items-center gap-1.5 text-sm text-ink-soft">
                    {treatment.invoice ? treatment.invoice.invoice_number : null}
                    <NotBilledBadge
                      count={
                        treatment.item_count > 0 &&
                        (!treatment.invoice || treatment.invoice.status === 'cancelled')
                          ? 1
                          : 0
                      }
                    />
                    <NotSentBadge status={treatment.invoice?.status} />
                  </span>
                </span>
                {treatment.patients.some((patient) => patient.treatment_reason) ? (
                  <span className="mt-0.5 block text-sm text-ink-soft">
                    {treatment.patients
                      .filter((patient) => patient.treatment_reason)
                      .map((patient) => `${patient.name}: ${patient.treatment_reason}`)
                      .join(' · ')}
                  </span>
                ) : null}
              </button>
            </li>
          ))}
          {(treatments.data ?? []).length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>

      <DuplicateDialog
        open={duplicateOpen}
        onOpenChange={setDuplicateOpen}
        pending={duplicate.isPending}
        onConfirm={(priceMode) =>
          duplicate.mutate({ id: appointmentId, data: { price_mode: priceMode } })
        }
      />
    </div>
  )
}

/** Asks whether the copy keeps the stored prices or takes today's (FR-026). */
function DuplicateDialog({
  open,
  onOpenChange,
  pending,
  onConfirm,
  title,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  pending: boolean
  onConfirm: (priceMode: PriceMode) => void
  title?: string
}) {
  const { t } = useTranslation()
  const [priceMode, setPriceMode] = useState<PriceMode>('verbatim')

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title ?? t('appointments.duplicate')}
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>{t('action.cancel')}</Button>
          <Button variant="primary" disabled={pending} onClick={() => onConfirm(priceMode)}>
            {t('action.duplicate')}
          </Button>
        </>
      }
    >
      <fieldset className="flex flex-col gap-2 border-0 p-0">
        <legend className="text-xs font-semibold text-ink-soft">
          {t('appointments.priceMode')}
        </legend>
        {(['verbatim', 'refresh'] as PriceMode[]).map((mode) => (
          <label
            key={mode}
            className="flex min-h-11 items-center gap-2 rounded-control border border-line px-3 sm:min-h-9"
          >
            <input
              type="radio"
              name="price-mode"
              value={mode}
              checked={priceMode === mode}
              onChange={() => setPriceMode(mode)}
              className="accent-ink"
            />
            <span className="text-sm">
              {mode === 'verbatim'
                ? t('appointments.priceModeVerbatim')
                : t('appointments.priceModeRefresh')}
            </span>
          </label>
        ))}
      </fieldset>
    </Dialog>
  )
}

function localIsoDate(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function localTime(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`
}
