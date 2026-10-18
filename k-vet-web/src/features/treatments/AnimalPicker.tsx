import { useQueryClient } from '@tanstack/react-query'
import { Command } from 'cmdk'
import { PawPrint, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  getGetTreatmentQueryKey,
  getListAppointmentsQueryKey,
  useAddTreatmentPatient,
  useListPatients,
  useRemoveTreatmentPatient,
} from '@/api/generated/endpoints'
import type { Treatment, TreatmentPatient } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

const SEARCH_DEBOUNCE_MS = 200

/** Refreshes what shows a treatment's animals: the treatment itself and the appointments list. */
function useAnimalsChanged(treatmentId: number) {
  const client = useQueryClient()
  return () =>
    Promise.all([
      client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) }),
      client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() }),
    ])
}

/**
 * The animals this customer could bring — the same set the server counts: complete, not
 * archived, alive (archived ones never come back from the list). With exactly one, that animal
 * is on the treatment by itself and stays there; with more, the picker is only offered while
 * one is left to add.
 */
export function useEligibleAnimals(treatment: Treatment) {
  const herd = useListPatients(
    { customer_id: treatment.customer_id ?? undefined },
    { query: { enabled: !treatment.frozen && treatment.customer_id != null } },
  )
  const eligible = (herd.data ?? []).filter((patient) => !patient.draft && !patient.date_of_death)
  const attached = new Set(treatment.patients.map((patient) => patient.patient_id))
  return {
    pending: herd.isPending,
    onlyAnimal: eligible.length === 1 ? eligible[0]?.id : undefined,
    leftToAdd: eligible.filter((patient) => !attached.has(patient.id)).length,
  }
}

/**
 * The search for a further animal of this visit. All animals of a treatment belong to its
 * customer (FR-027), so the search is limited to that customer's animals and the rule cannot
 * be broken by accident. `onAttached` gets the new record's id, to open its tab.
 */
export function AnimalPicker({
  treatment,
  onAttached,
}: {
  treatment: Treatment
  onAttached?: (recordId: number) => void
}) {
  const { t } = useTranslation()
  const changed = useAnimalsChanged(treatment.id)
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(term), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [term])

  const addPatient = useAddTreatmentPatient()
  const candidates = useListPatients(
    { q: debounced || undefined, customer_id: treatment.customer_id ?? undefined },
    { query: { enabled: !treatment.frozen } },
  )
  const attached = new Set(treatment.patients.map((patient) => patient.patient_id))
  const options = (candidates.data ?? []).filter(
    (patient) => !patient.draft && !patient.date_of_death && !attached.has(patient.id),
  )

  return (
    <Command
      shouldFilter={false}
      className="mt-3 rounded-card border border-line-strong bg-surface"
    >
      <Command.Input
        value={term}
        onValueChange={setTerm}
        placeholder={t('patients.title')}
        className="w-full rounded-t-card border-b border-line bg-transparent px-3 py-2.5 text-ink placeholder:text-ink-faint focus:outline-none"
      />
      <Command.List className="max-h-48 overflow-y-auto p-1">
        <Command.Empty className="px-2 py-3 text-sm text-ink-faint">
          {t('list.empty')}
        </Command.Empty>
        {options.map((patient) => (
          <Command.Item
            key={patient.id}
            value={String(patient.id)}
            onSelect={() => {
              setTerm('')
              addPatient.mutate(
                { id: treatment.id, data: { patient_id: patient.id } },
                {
                  onSuccess: async (updated) => {
                    await changed()
                    const record = updated.patients.find((entry) => entry.patient_id === patient.id)
                    if (record) onAttached?.(record.id)
                  },
                },
              )
            }}
            className={cn(
              'flex cursor-pointer items-center gap-2 rounded-control px-2 py-2 text-sm',
              'data-[selected=true]:bg-cream-soft',
            )}
          >
            <PawPrint className="size-4 shrink-0 text-rust" aria-hidden />
            <span className="min-w-0 flex-1 truncate text-ink">{patient.name}</span>
            <span className="shrink-0 text-xs text-ink-faint">{patient.customer_name ?? ''}</span>
          </Command.Item>
        ))}
      </Command.List>
    </Command>
  )
}

/**
 * Takes an animal off the visit, asked first: its record and lines go with it. Not offered for
 * the customer's only animal (the server refuses it too), nor once the invoice is accepted.
 */
export function RemoveAnimal({
  treatment,
  patient,
  onRemoved,
}: {
  treatment: Treatment
  patient: TreatmentPatient
  onRemoved?: () => void
}) {
  const { t } = useTranslation()
  const changed = useAnimalsChanged(treatment.id)
  const [confirming, setConfirming] = useState(false)
  const removePatient = useRemoveTreatmentPatient({
    mutation: {
      onSuccess: async () => {
        setConfirming(false)
        await changed()
        onRemoved?.()
      },
    },
  })

  // A line still attributed to the animal holds it on the visit (409).
  const error: unknown = removePatient.error
  const blocked = error instanceof ApiError && error.status === 409

  return (
    <>
      <Button size="small" variant="ghost" onClick={() => setConfirming(true)}>
        <Trash2 className="size-4" />
        {t('action.remove')}
      </Button>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={t('treatments.removePatient')}
        confirmLabel={t('action.remove')}
        busy={removePatient.isPending}
        onConfirm={() => removePatient.mutate({ id: treatment.id, patientId: patient.patient_id })}
      >
        {t('treatments.removePatientConfirm', { name: patient.name })}
      </ConfirmDialog>
      {blocked ? (
        <p className="basis-full text-xs text-danger" role="alert">
          {t('item.patientRequired')}
        </p>
      ) : null}
    </>
  )
}
