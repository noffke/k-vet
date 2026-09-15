import { useQueryClient } from '@tanstack/react-query'
import { Command } from 'cmdk'
import { PawPrint, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  getGetTreatmentQueryKey,
  useAddTreatmentPatient,
  useListPatients,
  useRemoveTreatmentPatient,
} from '@/api/generated/endpoints'
import type { Treatment } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

const SEARCH_DEBOUNCE_MS = 200

/**
 * The animals this treatment is about. All patients of a treatment must belong to one
 * customer (FR-027) — once the first is attached, the search is restricted to that
 * customer's animals, so the rule cannot be broken by accident.
 */
export function PatientAttach({ treatment }: { treatment: Treatment }) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(term), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [term])

  const refresh = () =>
    client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })

  const addPatient = useAddTreatmentPatient({ mutation: { onSuccess: refresh } })
  const removePatient = useRemoveTreatmentPatient({ mutation: { onSuccess: refresh } })
  /**
   * The animal a removal is being confirmed for. Its lines go with it, and the × sits inside
   * a small chip — an easy mis-tap on a phone.
   */
  const [pendingPatient, setPendingPatient] = useState<Treatment['patients'][number] | null>(null)

  const candidates = useListPatients(
    {
      q: debounced || undefined,
      customer_id: treatment.customer_id ?? undefined,
    },
    { query: { enabled: !treatment.frozen } },
  )

  const attachedIds = new Set(treatment.patients.map((patient) => patient.patient_id))
  const options = (candidates.data ?? []).filter(
    (patient) => !patient.draft && !attachedIds.has(patient.id),
  )

  const removeError: unknown = removePatient.error
  const removeBlocked = removeError instanceof ApiError && removeError.status === 409

  return (
    <section className="mt-5">
      <h2 className="text-base">{t('field.patient')}</h2>

      <ul className="mt-2 flex flex-wrap gap-2">
        {treatment.patients.map((patient) => (
          <li
            key={patient.patient_id}
            className="flex items-center gap-1.5 rounded-full border border-line bg-surface pl-3 pr-1 py-1"
          >
            <PawPrint className="size-3.5 text-rust" aria-hidden />
            <span className="text-sm text-ink">{patient.name}</span>
            {treatment.frozen ? null : (
              <Button
                size="icon"
                variant="ghost"
                aria-label={t('action.delete')}
                className="size-7 sm:size-7"
                onClick={() => setPendingPatient(patient)}
              >
                <X className="size-3.5" />
              </Button>
            )}
          </li>
        ))}
        {treatment.patients.length === 0 ? (
          <li className="text-sm text-ink-faint">{t('list.empty')}</li>
        ) : null}
      </ul>

      {removeBlocked ? (
        <p className="mt-2 text-xs text-danger" role="alert">
          {t('item.patientRequired')}
        </p>
      ) : null}

      <ConfirmDialog
        open={pendingPatient !== null}
        onOpenChange={(open) => {
          if (!open) setPendingPatient(null)
        }}
        title={t('treatments.removePatient')}
        confirmLabel={t('action.delete')}
        busy={removePatient.isPending}
        onConfirm={() => {
          if (pendingPatient) {
            removePatient.mutate({ id: treatment.id, patientId: pendingPatient.patient_id })
          }
        }}
      >
        {t('treatments.removePatientConfirm', { name: pendingPatient?.name ?? '' })}
      </ConfirmDialog>

      {treatment.frozen ? null : (
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
                  addPatient.mutate({ id: treatment.id, data: { patient_id: patient.id } })
                  setTerm('')
                }}
                className={cn(
                  'flex cursor-pointer items-center gap-2 rounded-control px-2 py-2 text-sm',
                  'data-[selected=true]:bg-cream-soft',
                )}
              >
                <PawPrint className="size-4 shrink-0 text-rust" aria-hidden />
                <span className="min-w-0 flex-1 truncate text-ink">{patient.name}</span>
                <span className="shrink-0 text-xs text-ink-faint">
                  {patient.customer_name ?? ''}
                </span>
              </Command.Item>
            ))}
          </Command.List>
        </Command>
      )}
    </section>
  )
}
