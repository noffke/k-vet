import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams, useSearch } from '@tanstack/react-router'
import { Copy, Trash2 } from 'lucide-react'
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  getGetAppointmentQueryKey,
  getGetTreatmentQueryKey,
  getListAppointmentsQueryKey,
  useDeleteAppointment,
  useDuplicateAppointment,
  useGetAppointment,
  useGetCustomer,
  useGetTreatment,
  usePatchAppointment,
} from '@/api/generated/endpoints'
import type { Appointment, PriceMode, Treatment } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { CustomerPicker } from '@/components/CustomerPicker'
import { DateInput } from '@/components/DateInput'
import { MapsLink } from '@/components/MapsLink'
import { PageHeader } from '@/components/PageHeader'
import {
  CalendarBadges,
  IncompleteBadge,
  NotBilledBadge,
  PdfStaleBadge,
} from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { TimeInput } from '@/components/TimeInput'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { TextAreaField } from '@/components/ui/field'
import { PhoneCalendarNotice } from '@/features/appointments/PhoneCalendarNotice'
import { RemindersCard } from '@/features/appointments/RemindersCard'
import { TreatmentSection } from '@/features/treatments/TreatmentSection'
import { useAutoSave } from '@/lib/autosave'
import { useOperatorConfig } from '@/lib/config'
import { todayIso } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { addressLine } from '@/lib/maps'
import { combineSaveStatus, type SaveStatus } from '@/lib/saveState'

/**
 * One appointment: date and time (auto-saved), a note, the customer — and its treatment, the
 * animals, lines and invoice of the visit, on the same page.
 *
 * Every appointment has exactly one treatment, made with it; a second invoice for the same
 * visit is a second appointment. The date defaults to today and the time is always typed by
 * the vet — the appointment stays "incomplete" until it has one (FR-025), and the treatment
 * takes animals and lines only once it is complete and has a customer.
 */
export function AppointmentDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/appointments/$id' })
  const { tier } = useSearch({ from: '/app/appointments/$id' })
  const appointmentId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()
  const { date: formatDate } = useLocaleFormat()

  const appointment = useGetAppointment(appointmentId)
  const treatmentId = appointment.data?.treatment_id ?? 0
  const treatment = useGetTreatment(treatmentId, { query: { enabled: treatmentId > 0 } })
  // Only for where to drive: the appointment itself carries the customer's name, not the address.
  const customerId = appointment.data?.customer_id ?? 0
  const customer = useGetCustomer(customerId, { query: { enabled: customerId > 0 } })
  const { default_country } = useOperatorConfig()
  const patchAppointment = usePatchAppointment()
  const [duplicateOpen, setDuplicateOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  // The open animal's record auto-saves on its own; the header shows one state for both.
  const [recordSave, setRecordSave] = useState<SaveStatus>({ state: 'idle', error: null })
  const selectRecord = useCallback(
    (recordId: number) =>
      void navigate({
        to: '/appointments/$id',
        params: { id: String(appointmentId) },
        search: { tier: recordId },
        replace: true,
      }),
    [navigate, appointmentId],
  )
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
      // The customer travels to the treatment, and its animal picker is limited by it.
      void client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(updated.treatment_id) })
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
            <SaveIndicator
              {...combineSaveStatus({ state: autoSave.state, error: autoSave.error }, recordSave)}
            />
            <Button onClick={() => setDuplicateOpen(true)}>
              <Copy className="size-4" />
              <span className="sr-only sm:not-sr-only">{t('action.duplicate')}</span>
            </Button>
            <Button
              variant="danger"
              // Named for what it deletes: on a phone it is an icon, and the treatment's lines
              // on the same page carry delete buttons of their own.
              aria-label={t('appointments.delete')}
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
        <NotBilledBadge count={record.unbilled ? 1 : 0} />
        {/* From the treatment, which every line edit refreshes — the appointment is not. */}
        <PdfStaleBadge stale={treatment.data?.pdf_stale} />
        <CalendarBadges appointment={record} />
      </PageHeader>

      <PhoneCalendarNotice
        appointment={record}
        onPickCustomer={(customerId) => {
          autoSave.set({ customer_id: customerId })
          void autoSave.flush()
        }}
        onShowAgain={() => {
          autoSave.set({ calendar_hidden: false })
          void autoSave.flush()
        }}
      />

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
          {/* Locked once something is entered on the treatment — a line, an invoice, a reason
              or finding; until then its animals are only the default and move with the
              customer. Locked while that is still being loaded, too: no answer yet is not
              "nothing entered". The server holds the same rule, files included. */}
          <CustomerPicker
            customerId={record.customer_id ?? null}
            customerName={record.customer_name ?? null}
            locked={treatment.isPending || hasContent(treatment.data)}
            onPick={(customerId) => {
              autoSave.set({ customer_id: customerId })
              void autoSave.flush()
            }}
            onClear={() => {
              autoSave.set({ customer_id: null })
              void autoSave.flush()
            }}
          />
          {/* Where the house call goes: the address, opening the drive there in Google Maps. */}
          {record.customer_id && customer.data?.id === record.customer_id ? (
            <MapsLink
              className="mt-1.5"
              address={{
                street: customer.data.home_street,
                zip: customer.data.home_zip,
                city: customer.data.home_city,
                country: customer.data.home_country ?? default_country,
              }}
            >
              {addressLine({
                street: customer.data.home_street,
                zip: customer.data.home_zip,
                city: customer.data.home_city,
              })}
            </MapsLink>
          ) : null}
          {/* The one refusal the lock above cannot foresee: files on an animal's record. */}
          {autoSave.fieldErrors.customer_id ? (
            <p className="mt-1.5 text-xs text-danger" role="alert">
              {t(autoSave.fieldErrors.customer_id)}
            </p>
          ) : null}
        </div>
      </section>

      {/* Before the treatment: what to bring, written down when the customer phones. */}
      <RemindersCard
        appointmentId={appointmentId}
        treatmentId={record.treatment_id}
        startsAt={record.starts_at}
      />

      {/* What the treatment needs before anything goes onto it — a date to bill under and
          a customer whose animals are on offer (issues.md 7). The server holds the same rule. */}
      {record.draft ? (
        <p className="mt-6 text-sm text-ink-faint">
          {t('record.incompleteWithFields', { fields: t('field.time') })}
        </p>
      ) : !record.customer_id ? (
        <p className="mt-6 text-sm text-ink-faint">{t('treatment.customerRequired')}</p>
      ) : (
        <TreatmentSection
          treatmentId={record.treatment_id}
          activeRecordId={tier}
          onSelectRecord={selectRecord}
          onSaveState={setRecordSave}
        />
      )}

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

/** Whether anything has been entered on the treatment beyond its animals. */
function hasContent(treatment: Treatment | undefined): boolean {
  if (!treatment) return false
  return (
    treatment.item_count > 0 ||
    treatment.invoice != null ||
    treatment.patients.some((patient) => patient.treatment_reason || patient.finding)
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
