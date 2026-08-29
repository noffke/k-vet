import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import {
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Pill,
  Plus,
  Stethoscope,
  Trash2,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  getGetTreatmentQueryKey,
  getListTreatmentItemsQueryKey,
  useApplyTemplate,
  useBulkDeleteTreatmentItems,
  useCreateTreatmentItem,
  useDeleteTreatmentItem,
  useMoveTreatmentItem,
  usePatchTreatmentItem,
} from '@/api/generated/endpoints'
import type { MoveDirection, PickerItem, Treatment, TreatmentItem } from '@/api/generated/model'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/** How long the undo stays on screen. Sonner's 4 s default is too quick to read and decide. */
const UNDO_MS = 10_000

interface LineEditorProps {
  treatment: Treatment
  items: TreatmentItem[]
  /** Frozen treatments (accepted invoice) render read-only. */
  readOnly: boolean
}

/**
 * The billing lines of a treatment — the screen the practice spends its day in.
 *
 * Prices are pinned when a line is added and stay editable per line; the lots a drug line
 * was dispensed from are shown underneath it, because that is what makes a batch
 * traceable later.
 */
export function LineEditor({ treatment, items, readOnly }: LineEditorProps) {
  const { t } = useTranslation()
  const { money } = useLocaleFormat()

  const loose = items.filter((item) => item.patient_treatment_id === null)
  // The bucket is for what covers the visit rather than an animal — a Wegegeld shared by two
  // of them. Rare, so it only appears once it holds something, or on request.
  const [bucketOpen, setBucketOpen] = useState(false)
  const showBucket = loose.length > 0 || bucketOpen || treatment.patients.length === 0

  return (
    <section className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-base">{t('treatments.items')}</h2>
        <span className="flex items-center gap-3">
          {readOnly || showBucket ? null : (
            <Button size="small" variant="ghost" onClick={() => setBucketOpen(true)}>
              <Plus className="size-4" />
              {t('treatments.withoutPatient')}
            </Button>
          )}
          <span className="numeric text-sm font-semibold text-ink">
            {t('treatments.total')}: {money(treatment.total_gross)}
          </span>
        </span>
      </div>

      {treatment.patients.map((patient) => (
        <PositionGroup
          key={patient.id}
          treatment={treatment}
          heading={
            <Link
              to="/patient-treatments/$id"
              params={{ id: String(patient.id) }}
              className="hover:underline"
            >
              {patient.name}
            </Link>
          }
          patientTreatmentId={patient.id}
          items={items.filter((item) => item.patient_treatment_id === patient.id)}
          readOnly={readOnly}
        />
      ))}

      {showBucket ? (
        <PositionGroup
          treatment={treatment}
          heading={<span className="text-ink-soft">{t('treatments.withoutPatient')}</span>}
          patientTreatmentId={null}
          items={loose}
          readOnly={readOnly}
        />
      ) : null}
    </section>
  )
}

interface PositionGroupProps {
  treatment: Treatment
  heading: ReactNode
  /** The animal these lines belong to, or null for the visit's own. */
  patientTreatmentId: number | null
  items: TreatmentItem[]
  readOnly: boolean
}

/**
 * One animal's positions: its own search box, its own order. A Behandlungsgruppe is offered
 * only inside an animal's group — its lines are clinical and belong to one.
 */
export function PositionGroup({
  treatment,
  heading,
  patientTreatmentId,
  items,
  readOnly,
}: PositionGroupProps) {
  const { t } = useTranslation()
  const { money, quantity: formatQuantity, percent, currencySymbol } = useLocaleFormat()
  const client = useQueryClient()

  const invalidate = async () => {
    await client.invalidateQueries({ queryKey: getListTreatmentItemsQueryKey(treatment.id) })
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
  }
  const mutation = { onSuccess: invalidate }

  const addItem = useCreateTreatmentItem({ mutation })
  const patchItem = usePatchTreatmentItem({ mutation })
  const moveItem = useMoveTreatmentItem({ mutation })
  const deleteItem = useDeleteTreatmentItem({ mutation })
  const bulkDelete = useBulkDeleteTreatmentItems({ mutation })
  const applyTemplate = useApplyTemplate({ mutation })

  /**
   * A Behandlungsgruppe adds several positions at once, and the groups sit directly above the
   * individual items in the picker — so the wrong one is easy to click. Offer to take it back
   * for as long as that mistake takes to notice.
   *
   * Which lines to remove is read off the response, which carries the *whole* treatment: the
   * animal has to be matched as well, because the other animals' lines are absent from `items`
   * and would otherwise look newly added.
   */
  const offerUndo = (applied: TreatmentItem[], groupName: string) => {
    const before = new Set(items.map((item) => item.id))
    const added = applied
      .filter((item) => item.patient_treatment_id === patientTreatmentId)
      .filter((item) => !before.has(item.id))
    if (added.length === 0) return

    toast(t('treatments.groupAdded', { count: added.length, name: groupName }), {
      duration: UNDO_MS,
      action: {
        label: t('action.undo'),
        onClick: () =>
          bulkDelete.mutate({
            id: treatment.id,
            data: { item_ids: added.map((item) => item.id) },
          }),
      },
    })
  }

  const pick = (item: PickerItem) => {
    addItem.mutate({
      id: treatment.id,
      data:
        item.kind === 'drug_packaging'
          ? {
              kind: 'drug_packaging',
              drug_packaging_id: item.id,
              quantity: '1',
              patient_treatment_id: patientTreatmentId,
            }
          : {
              kind: 'service',
              service_id: item.id,
              quantity: '1',
              patient_treatment_id: patientTreatmentId,
            },
    })
  }

  const move = (itemId: number, direction: MoveDirection) =>
    moveItem.mutate({ id: itemId, data: { direction } })

  return (
    <div className="mt-4">
      <h3 className="text-sm font-semibold text-ink">{heading}</h3>

      {readOnly ? null : (
        <div className="mt-2">
          <ItemPicker
            onPick={pick}
            onPickTemplate={
              patientTreatmentId === null
                ? undefined
                : (template) =>
                    applyTemplate.mutate(
                      {
                        id: treatment.id,
                        data: {
                          template_id: template.id,
                          patient_treatment_id: patientTreatmentId,
                        },
                      },
                      // Here rather than on the hook: this is where the group that was
                      // clicked is known, and its name is what the undo has to say.
                      { onSuccess: (applied) => offerUndo(applied, template.name ?? '') },
                    )
            }
            disabled={addItem.isPending}
          />
        </div>
      )}

      <ul className="mt-2 flex flex-col gap-2">
        {items.map((item, index) => (
          <li key={item.id} className="rounded-card border border-line bg-surface p-3">
            <div className="flex items-start gap-2">
              {item.kind === 'drug_packaging' ? (
                <Pill className="mt-0.5 size-4 shrink-0 text-rust" aria-hidden />
              ) : (
                <Stethoscope className="mt-0.5 size-4 shrink-0 text-ink-soft" aria-hidden />
              )}
              <div className="min-w-0 flex-1">
                <input
                  aria-label={t('field.name')}
                  defaultValue={item.name}
                  disabled={readOnly}
                  onBlur={(event) => {
                    if (event.target.value !== item.name) {
                      patchItem.mutate({ id: item.id, data: { name: event.target.value } })
                    }
                  }}
                  className="w-full border-0 bg-transparent p-0 font-medium text-ink focus:outline-none disabled:text-ink-soft"
                />
                <p className="text-xs text-ink-faint">
                  {item.got_number
                    ? `${item.got_analogous ? '§8' : 'GOT'} ${item.got_number} · `
                    : ''}
                  {percent(item.vat_percent)} {t('field.vat')}
                </p>
              </div>
              <span className="numeric shrink-0 font-semibold text-ink">
                {money(item.line_gross)}
              </span>
            </div>

            <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
              <NumberInput
                label={t('field.quantity')}
                value={item.quantity}
                disabled={readOnly}
                onChange={(value) =>
                  value && patchItem.mutate({ id: item.id, data: { quantity: value } })
                }
              />
              <NumberInput
                label={t('field.priceNet')}
                value={item.price_net}
                unit={currencySymbol}
                money
                hint={`${t('field.priceGross')}: ${money(item.price_gross)}`}
                disabled={readOnly}
                onChange={(value) =>
                  value && patchItem.mutate({ id: item.id, data: { price_net: value } })
                }
              />
              {item.kind === 'service' ? (
                <NumberInput
                  label={t('field.factor')}
                  value={item.factor ?? null}
                  decimals={3}
                  unit="%"
                  disabled={readOnly}
                  onChange={(value) =>
                    value && patchItem.mutate({ id: item.id, data: { factor: value } })
                  }
                />
              ) : null}
              {item.travel_expenses ? (
                <>
                  <NumberInput
                    label={t('field.km')}
                    value={item.km ?? null}
                    unit="km"
                    disabled={readOnly}
                    hint={t('treatments.kmHint')}
                    onChange={(value) =>
                      value && patchItem.mutate({ id: item.id, data: { km: value } })
                    }
                  />
                  <NumberInput
                    label={t('field.kmMultiplier')}
                    value={item.km_multiplier ?? null}
                    decimals={3}
                    disabled={readOnly}
                    onChange={(value) =>
                      value && patchItem.mutate({ id: item.id, data: { km_multiplier: value } })
                    }
                  />
                </>
              ) : null}
              {/* Moving a line to the animal it belongs to, when it was entered under the
                  wrong one. A drug line cannot leave every animal — that is its traceability. */}
              {treatment.patients.length > 1 || item.patient_treatment_id === null ? (
                <label className="flex flex-col gap-1">
                  <span className="text-xs font-semibold text-ink-soft">{t('field.patient')}</span>
                  <select
                    value={item.patient_treatment_id ?? ''}
                    disabled={readOnly}
                    onChange={(event) =>
                      patchItem.mutate({
                        id: item.id,
                        data: {
                          patient_treatment_id: event.target.value
                            ? Number(event.target.value)
                            : null,
                        },
                      })
                    }
                    className="rounded-control border border-line-strong bg-surface px-2 py-2 min-h-11 sm:min-h-9"
                  >
                    {item.kind === 'service' ? (
                      <option value="">{t('treatments.withoutPatient')}</option>
                    ) : null}
                    {treatment.patients.map((patient) => (
                      <option key={patient.id} value={patient.id}>
                        {patient.name}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
            </div>

            {item.kind === 'drug_packaging' ? (
              // Ad-hoc Umwidmung — an eye preparation used in an ear. Documentation only: it is
              // recorded on this line and never touches the price.
              <div className="mt-2">
                <CheckboxField
                  label={t('treatments.redesignation')}
                  defaultChecked={item.redesignation}
                  disabled={readOnly}
                  onChange={(event) =>
                    patchItem.mutate({
                      id: item.id,
                      data: { redesignation: event.target.checked },
                    })
                  }
                />
                <p className="text-xs text-ink-faint">{t('treatments.redesignationHint')}</p>
              </div>
            ) : null}

            {item.lots.length > 0 ? (
              <p className="mt-2 text-xs text-ink-faint">
                {t('treatments.lots')}:{' '}
                {item.lots
                  .map(
                    (lot) =>
                      `${lot.batch_number ?? lot.lot_id} (${formatQuantity(lot.quantity)} ${
                        item.unit ?? ''
                      })`,
                  )
                  .join(', ')}
              </p>
            ) : null}

            {readOnly ? null : (
              <div className="mt-2 flex justify-end gap-1">
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveTop')}
                  disabled={index === 0}
                  onClick={() => move(item.id, 'top')}
                >
                  <ChevronsUp className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveUp')}
                  disabled={index === 0}
                  onClick={() => move(item.id, 'up')}
                >
                  <ChevronUp className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveDown')}
                  disabled={index === items.length - 1}
                  onClick={() => move(item.id, 'down')}
                >
                  <ChevronDown className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveBottom')}
                  disabled={index === items.length - 1}
                  onClick={() => move(item.id, 'bottom')}
                >
                  <ChevronsDown className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.delete')}
                  onClick={() => deleteItem.mutate({ id: item.id })}
                >
                  <Trash2 className="size-4 text-danger" />
                </Button>
              </div>
            )}
          </li>
        ))}
        {items.length === 0 ? (
          <li className="rounded-card border border-dashed border-line-strong px-4 py-4 text-center text-sm text-ink-faint">
            {t('list.empty')}
          </li>
        ) : null}
      </ul>
    </div>
  )
}
