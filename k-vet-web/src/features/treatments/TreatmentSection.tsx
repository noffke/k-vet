import * as Tabs from '@radix-ui/react-tabs'
import { Link } from '@tanstack/react-router'
import { Plus, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useGetTreatment, useListTreatmentItems } from '@/api/generated/endpoints'
import type { Treatment, TreatmentItem, TreatmentPatient } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { WarningBanner } from '@/components/WarningBanner'
import { AnimalPicker, RemoveAnimal, useEligibleAnimals } from '@/features/treatments/AnimalPicker'
import { AnimalRecord } from '@/features/treatments/AnimalRecord'
import { InvoicePanel } from '@/features/treatments/InvoicePanel'
import { PositionGroup } from '@/features/treatments/LineEditor'
import { PdfStaleBanner } from '@/features/treatments/PdfStaleBanner'
import { useLocaleFormat } from '@/lib/locale'
import type { SaveStatus } from '@/lib/saveState'
import { cn } from '@/lib/utils'

interface TreatmentSectionProps {
  treatmentId: number
  /** The open animal's record (`?tier=`); the first animal when absent or gone. */
  activeRecordId?: number | undefined
  onSelectRecord: (recordId: number) => void
  onSaveState: (status: SaveStatus) => void
}

/**
 * The treatment of an appointment, on the appointment's page: one tab per animal, each with
 * that animal's record and lines, then the visit's total and invoice.
 *
 * A second animal is a second tab, not a different page — the vet starts writing in the first
 * animal's tab straight away, whether it stays alone or not.
 */
export function TreatmentSection(props: TreatmentSectionProps) {
  const { t } = useTranslation()
  const treatment = useGetTreatment(props.treatmentId)
  const items = useListTreatmentItems(props.treatmentId)

  if (treatment.isPending) return <p className="mt-6 text-sm text-ink-faint">{t('list.loading')}</p>
  if (!treatment.data) return <p className="mt-6 text-sm text-danger">{t('error.notFound')}</p>
  return <TreatmentBody {...props} treatment={treatment.data} items={items.data ?? []} />
}

function TreatmentBody({
  treatment,
  items,
  activeRecordId,
  onSelectRecord,
  onSaveState,
}: TreatmentSectionProps & { treatment: Treatment; items: TreatmentItem[] }) {
  const { t } = useTranslation()
  const { money } = useLocaleFormat()
  const eligible = useEligibleAnimals(treatment)
  const [adding, setAdding] = useState(false)

  const patients = treatment.patients
  const active = patients.find((patient) => patient.id === activeRecordId) ?? patients[0]
  // The visit's own positions: the house-call fee, Wegegeld — independent of any one animal.
  const general = items.filter((item) => item.patient_treatment_id === null)
  const canAdd = !treatment.frozen && !eligible.pending && eligible.leftToAdd > 0

  return (
    <section className="mt-6">
      <PdfStaleBanner treatment={treatment} />
      {active === undefined ? (
        // No animal yet: a customer with several, none picked. The visit's own positions —
        // the services marked "pro Termin" — can be written down already; nothing else can.
        <>
          <h2 className="text-base">{t('field.patient')}</h2>
          {canAdd ? <AnimalPicker treatment={treatment} onAttached={onSelectRecord} /> : null}
          <PositionGroup
            treatment={treatment}
            heading={t('treatments.perVisit')}
            patientTreatmentId={null}
            items={general}
            readOnly={treatment.frozen}
          />
        </>
      ) : (
        <Tabs.Root
          value={String(active.id)}
          onValueChange={(value) => onSelectRecord(Number(value))}
        >
          <div className="flex items-end gap-2 border-b border-line">
            {/* Scrolls sideways inside itself with many animals — never the page (helpers.ts).
                Never up and down: `overflow-x` alone turns `overflow-y` to auto as well, and
                the tabs then grew a scrollbar of their own. */}
            <Tabs.List
              aria-label={t('field.patients')}
              className="flex min-w-0 flex-1 gap-1 overflow-x-auto overflow-y-hidden"
            >
              {patients.map((patient) => (
                <Tabs.Trigger
                  key={patient.id}
                  value={String(patient.id)}
                  className={cn(
                    'flex min-h-11 shrink-0 items-center gap-1.5 border-b-2 border-transparent px-3 text-sm font-medium text-ink-soft sm:min-h-9',
                    'hover:text-ink data-[state=active]:border-rust data-[state=active]:text-rust',
                  )}
                >
                  {/* A warning sits in its animal's tab; the mark says so while it is closed. */}
                  {patient.warning_remark ? (
                    <TriangleAlert
                      className="size-3.5 text-danger"
                      role="img"
                      aria-label={t('field.warningRemark')}
                    />
                  ) : null}
                  {patient.name}
                </Tabs.Trigger>
              ))}
            </Tabs.List>
            {canAdd ? (
              <Button
                size="small"
                variant="ghost"
                aria-expanded={adding}
                onClick={() => setAdding((open) => !open)}
              >
                <Plus className="size-4" />
                {t('treatments.addPatient')}
              </Button>
            ) : null}
          </div>

          {adding && canAdd ? (
            <AnimalPicker
              treatment={treatment}
              onAttached={(recordId) => {
                setAdding(false)
                onSelectRecord(recordId)
              }}
            />
          ) : null}

          {/* Only the open tab is mounted: the visit's shared lines are never on screen twice. */}
          {patients.map((patient) => (
            <Tabs.Content key={patient.id} value={String(patient.id)} className="pt-4">
              <AnimalPanel
                treatment={treatment}
                patient={patient}
                items={items}
                general={general}
                removable={
                  !treatment.frozen &&
                  !eligible.pending &&
                  patient.patient_id !== eligible.onlyAnimal
                }
                onRemoved={() => {
                  const next = patients.find((other) => other.id !== patient.id)
                  if (next) onSelectRecord(next.id)
                }}
                onSaveState={onSaveState}
              />
            </Tabs.Content>
          ))}
        </Tabs.Root>
      )}

      <p className="numeric mt-6 text-right text-sm font-semibold text-ink">
        {t('treatments.total')}: {money(treatment.total_gross)}
      </p>
      <InvoicePanel treatment={treatment} hasItems={items.length > 0} />
    </section>
  )
}

/**
 * One animal's tab: its warning, its record, its lines — and below them the visit's own
 * positions, marked "pro Termin", which every tab shows: they are the same lines, billed once,
 * and each tab tells the vet whether the house call is already entered. They are added from the
 * animal's own search box: a service marked "pro Termin" goes there by itself.
 */
function AnimalPanel({
  treatment,
  patient,
  items,
  general,
  removable,
  onRemoved,
  onSaveState,
}: {
  treatment: Treatment
  patient: TreatmentPatient
  items: TreatmentItem[]
  general: TreatmentItem[]
  removable: boolean
  onRemoved: () => void
  onSaveState: (status: SaveStatus) => void
}) {
  const { t } = useTranslation()

  return (
    <div>
      {patient.warning_remark ? (
        <WarningBanner className="mb-4" title={patient.name} remark={patient.warning_remark} />
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <Link
          to="/patients/$id"
          params={{ id: String(patient.patient_id) }}
          className="text-base font-semibold text-ink hover:underline"
        >
          {patient.name}
        </Link>
        {removable ? (
          <RemoveAnimal
            treatment={treatment}
            patient={patient}
            lineCount={items.filter((item) => item.patient_treatment_id === patient.id).length}
            onRemoved={onRemoved}
          />
        ) : null}
      </div>

      <AnimalRecord
        key={patient.id}
        recordId={patient.id}
        changedSincePdf={treatment.pdf_changes?.changed_records.includes(patient.id) ?? false}
        onSaveState={onSaveState}
      />

      <PositionGroup
        treatment={treatment}
        heading={t('treatments.items')}
        patientTreatmentId={patient.id}
        items={items.filter((item) => item.patient_treatment_id === patient.id)}
        readOnly={treatment.frozen}
      />

      {general.length > 0 ? (
        <PositionGroup
          treatment={treatment}
          heading={t('treatments.perVisit')}
          patientTreatmentId={null}
          items={general}
          readOnly={treatment.frozen}
          showPicker={false}
        />
      ) : null}
    </div>
  )
}
