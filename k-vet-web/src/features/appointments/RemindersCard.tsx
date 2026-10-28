import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { BriefcaseMedical, PackageX, Pill, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  getGetDashboardQueryKey,
  getGetTreatmentQueryKey,
  getListAppointmentsQueryKey,
  getListRemindersQueryKey,
  getListTreatmentItemsQueryKey,
  getPickerItemsQueryKey,
  patchReminder,
  useCreateReminder,
  useDeleteReminder,
  useGetTreatment,
  useListReminders,
  useTakeReminder,
} from '@/api/generated/endpoints'
import type { Reminder, Treatment } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { TextField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'
import { refusalKey } from '@/lib/refusal'
import { scheduleOf } from '@/lib/schedule'
import { cn } from '@/lib/utils'

interface RemindersCardProps {
  appointmentId: number
  treatmentId: number
  /** The appointment's start, for the link to that day's Packliste. */
  startsAt: string | null | undefined
}

/**
 * "Mitbringen": drugs a customer has asked the vet to bring — more of a regular medication,
 * phoned in — kept on the appointment instead of on a slip of paper. A reminder dispenses
 * nothing and names no animal; on the day "Als Position übernehmen" makes it a line of the
 * treatment, booked from the lots like a picked drug.
 */
export function RemindersCard({ appointmentId, treatmentId, startsAt }: RemindersCardProps) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const reminders = useListReminders(appointmentId)
  const treatment = useGetTreatment(treatmentId)
  const refused = (error: unknown) => toast.error(t(refusalKey(error)))
  const changed = async () => {
    await client.invalidateQueries({ queryKey: getListRemindersQueryKey(appointmentId) })
    await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
  }
  const create = useCreateReminder({ mutation: { onSuccess: changed, onError: refused } })
  const rows = reminders.data ?? []
  const day = startsAt ? scheduleOf(startsAt).date : null

  return (
    <section className="mt-6 rounded-card border border-line bg-surface p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="flex items-center gap-1.5 text-sm font-semibold text-ink">
          <BriefcaseMedical className="size-4 text-rust" aria-hidden />
          {t('reminders.title')}
        </h2>
        {day && rows.some((row) => !row.taken_item_id) ? (
          <Link
            to="/packing-list"
            search={{ date: day }}
            className="text-sm text-rust underline-offset-2 hover:underline"
          >
            {t('reminders.packingListLink')}
          </Link>
        ) : null}
      </div>
      <p className="mt-0.5 text-xs text-ink-faint">{t('reminders.hint')}</p>
      <div className="mt-2">
        <ItemPicker
          drugsOnly
          placeholder={t('reminders.pickerPlaceholder')}
          disabled={create.isPending}
          onPick={(item) =>
            create.mutate({
              id: appointmentId,
              data: { drug_packaging_id: item.id, quantity: '1' },
            })
          }
        />
      </div>
      {rows.length > 0 ? (
        <ul className="mt-3 flex flex-col gap-2">
          {rows.map((reminder) => (
            <ReminderRow
              key={reminder.id}
              reminder={reminder}
              treatment={treatment.data}
              onChanged={changed}
            />
          ))}
        </ul>
      ) : null}
    </section>
  )
}

function ReminderRow({
  reminder,
  treatment,
  onChanged,
}: {
  reminder: Reminder
  treatment: Treatment | undefined
  onChanged: () => Promise<void>
}) {
  const { t } = useTranslation()
  const { quantity } = useLocaleFormat()
  const client = useQueryClient()
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [choosing, setChoosing] = useState(false)
  // Ticked at once, saved behind it.
  const [packed, setPacked] = useState(reminder.packed)
  useEffect(() => setPacked(reminder.packed), [reminder.packed])
  const refused = (error: unknown) => toast.error(t(refusalKey(error)))
  const taken = Boolean(reminder.taken_item_id)

  const autoSave = useAutoSave<Reminder>({
    save: (patch) => patchReminder(reminder.id, patch),
    onSaved: () => void onChanged(),
  })
  const remove = useDeleteReminder({
    mutation: {
      onSuccess: async () => {
        setConfirmDelete(false)
        await onChanged()
      },
      onError: refused,
    },
  })
  const take = useTakeReminder({
    mutation: {
      onSuccess: async () => {
        setChoosing(false)
        toast(t('reminders.taken', { name: reminder.name }))
        await onChanged()
        if (treatment) {
          await client.invalidateQueries({ queryKey: getListTreatmentItemsQueryKey(treatment.id) })
          await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
        }
        await client.invalidateQueries({ queryKey: getPickerItemsQueryKey() })
        await client.invalidateQueries({ queryKey: getGetDashboardQueryKey() })
      },
      onError: refused,
    },
  })

  const animals = treatment?.patients ?? []
  // A position needs an animal, an open treatment, and an invoice not yet released.
  const takeBlocked = !treatment || treatment.frozen || animals.length === 0
  const takeTo = (patientTreatmentId?: number) =>
    take.mutate({ id: reminder.id, data: { patient_treatment_id: patientTreatmentId } })
  const size = reminder.size ? `${quantity(reminder.size)} ${reminder.unit ?? ''}`.trim() : ''

  return (
    <li
      className={cn(
        'rounded-card border border-line p-3',
        taken ? 'bg-sunken' : 'bg-surface',
        packed && !taken && 'border-l-4 border-l-sage-deep',
      )}
    >
      <div className="flex items-start gap-2">
        <Pill className="mt-0.5 size-4 shrink-0 text-rust" aria-hidden />
        <div className="min-w-0 flex-1">
          <p className="font-medium text-ink">
            {reminder.name}
            {size ? <span className="text-ink-soft"> · {size}</span> : null}
          </p>
          {taken ? (
            <p className="text-xs font-semibold text-sage-deep">{t('reminders.takenMark')}</p>
          ) : reminder.stock_short ? (
            <p className="flex items-center gap-1 text-xs font-semibold text-danger">
              <PackageX className="size-3.5" aria-hidden />
              {t('reminders.stockShort')}
            </p>
          ) : null}
        </div>
        {taken ? null : (
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('action.delete')}
            onClick={() => setConfirmDelete(true)}
          >
            <Trash2 className="size-4 text-danger" />
          </Button>
        )}
      </div>

      {taken ? null : (
        <>
          <div className="mt-2 grid gap-2 sm:grid-cols-[8rem_1fr]">
            <NumberInput
              label={t('field.quantity')}
              value={reminder.quantity}
              step={1}
              min={1}
              onChange={(value) => {
                if (value) {
                  autoSave.set({ quantity: value })
                }
              }}
            />
            <TextField
              label={t('reminders.note')}
              defaultValue={reminder.note ?? ''}
              placeholder={t('reminders.notePlaceholder')}
              error={autoSave.fieldErrors.note ? t(autoSave.fieldErrors.note) : undefined}
              onChange={(event) => autoSave.set({ note: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
          </div>
          <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
            <label className="flex min-h-11 items-center gap-2 text-sm text-ink sm:min-h-9">
              <input
                type="checkbox"
                className="size-4 accent-ink"
                checked={packed}
                onChange={(event) => {
                  setPacked(event.target.checked)
                  autoSave.set({ packed: event.target.checked })
                  void autoSave.flush()
                }}
              />
              {t('reminders.packed')}
            </label>
            <Button
              disabled={takeBlocked || take.isPending}
              title={takeBlocked ? t('reminders.takeBlocked') : undefined}
              onClick={() => (animals.length > 1 ? setChoosing(true) : takeTo())}
            >
              {t('reminders.take')}
            </Button>
          </div>
        </>
      )}

      <Dialog
        open={choosing}
        onOpenChange={(open) => {
          if (!take.isPending) setChoosing(open)
        }}
        title={t('reminders.takeFor', { name: reminder.name })}
        footer={
          <Button disabled={take.isPending} onClick={() => setChoosing(false)}>
            {t('action.cancel')}
          </Button>
        }
      >
        <ul className="flex flex-col gap-2">
          {animals.map((animal) => (
            <li key={animal.id}>
              <Button
                className="w-full"
                disabled={take.isPending}
                onClick={() => takeTo(animal.id)}
              >
                {animal.name}
              </Button>
            </li>
          ))}
        </ul>
      </Dialog>
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={t('reminders.delete')}
        confirmLabel={t('action.delete')}
        variant="danger"
        busy={remove.isPending}
        onConfirm={() => remove.mutate({ id: reminder.id })}
      >
        {t('reminders.deleteConfirm', { name: reminder.name })}
      </ConfirmDialog>
    </li>
  )
}
