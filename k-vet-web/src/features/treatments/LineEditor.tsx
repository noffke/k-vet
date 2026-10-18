import { useQueryClient } from '@tanstack/react-query'
import {
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Pill,
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
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/** How long the undo stays on screen. Sonner's 4 s default is too quick to read and decide. */
const UNDO_MS = 10_000

interface PositionGroupProps {
  treatment: Treatment
  heading: ReactNode
  /** The animal these lines belong to, or null for the visit's own. */
  patientTreatmentId: number | null
  items: TreatmentItem[]
  readOnly: boolean
  /**
   * Whether the group has a search box of its own. The visit's lines have one only while no
   * animal is on the visit; after that a per-visit service is picked in an animal's tab and
   * lands among them by itself.
   */
  showPicker?: boolean
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
  showPicker = true,
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
  /**
   * The line a removal is being confirmed for. Deleting sits in a row of five icon buttons,
   * next to "move down" — on a phone that is one thumb's width from a position the vet meant
   * to reorder, and a removed drug line takes its dispense with it.
   */
  const [pendingRemoval, setPendingRemoval] = useState<TreatmentItem | null>(null)
  const bulkDelete = useBulkDeleteTreatmentItems({ mutation })
  const applyTemplate = useApplyTemplate({ mutation })

  /**
   * A Behandlungsgruppe adds several positions at once, and the groups sit directly above the
   * individual items in the picker — so the wrong one is easy to click. Offer to take it back
   * for as long as that mistake takes to notice.
   *
   * Which lines to remove is read off the response, which carries the *whole* treatment, by
   * comparing it with the whole treatment as it was before — not with this animal's lines: a
   * group's per-visit positions land among the visit's, and would escape the undo.
   */
  const offerUndo = (applied: TreatmentItem[], before: Set<number> | null, groupName: string) => {
    // Without knowing what was there, every line would look new — better no undo than that.
    if (before === null) return
    const added = applied.filter((item) => !before.has(item.id))
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

  /**
   * A picked position is appended to the end of the list, which on a treatment of any length is
   * below the fold — so nothing on screen moves and the vet cannot tell the click landed
   * (issues.md 7). Name what was added rather than just confirming that something was.
   */
  const pick = (item: PickerItem) => {
    // The visit's own group names no animal; the server files a per-visit service under the
    // visit whichever animal is named.
    const patient_treatment_id = patientTreatmentId ?? undefined
    // Picked in an animal's tab, a per-visit service lands in the "pro Termin" lines below,
    // not among the animal's — the toast says where it went.
    const elsewhere = item.kind === 'service' && item.per_visit && patientTreatmentId !== null
    addItem.mutate(
      {
        id: treatment.id,
        data:
          item.kind === 'drug_packaging'
            ? {
                kind: 'drug_packaging',
                drug_packaging_id: item.id,
                quantity: '1',
                patient_treatment_id,
              }
            : { kind: 'service', service_id: item.id, quantity: '1', patient_treatment_id },
      },
      {
        onSuccess: () =>
          toast(
            t(elsewhere ? 'treatments.itemAddedPerVisit' : 'treatments.itemAdded', {
              name: item.name,
            }),
          ),
      },
    )
  }

  const move = (itemId: number, direction: MoveDirection) =>
    moveItem.mutate({ id: itemId, data: { direction } })

  const pickerBlock =
    readOnly || !showPicker ? null : (
      <div className="mt-2">
        <ItemPicker
          onPick={pick}
          perVisitOnly={patientTreatmentId === null}
          onPickTemplate={
            patientTreatmentId === null
              ? undefined
              : (template) => {
                  // What the treatment holds right now, before the group adds to it.
                  const current = client.getQueryData<TreatmentItem[]>(
                    getListTreatmentItemsQueryKey(treatment.id),
                  )
                  const before = current ? new Set(current.map((item) => item.id)) : null
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
                    { onSuccess: (applied) => offerUndo(applied, before, template.name ?? '') },
                  )
                }
          }
          disabled={addItem.isPending}
        />
      </div>
    )

  return (
    <div className="mt-4">
      <h3 className="text-sm font-semibold text-ink">{heading}</h3>
      {patientTreatmentId === null && !readOnly && showPicker ? (
        <p className="mt-0.5 text-xs text-ink-faint">{t('treatments.withoutPatientHint')}</p>
      ) : null}

      {pickerBlock}

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
                  wrong one. Never between an animal and the visit: that is the service's to
                  say ("pro Termin"), and the visit's lines have no animal to move between. */}
              {treatment.patients.length > 1 && item.patient_treatment_id !== null ? (
                <label className="flex flex-col gap-1">
                  <span className="text-xs font-semibold text-ink-soft">{t('field.patient')}</span>
                  <select
                    value={item.patient_treatment_id}
                    disabled={readOnly}
                    onChange={(event) =>
                      patchItem.mutate({
                        id: item.id,
                        data: { patient_treatment_id: Number(event.target.value) },
                      })
                    }
                    className="rounded-control border border-line-strong bg-surface px-2 py-2 min-h-11 sm:min-h-9"
                  >
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
                  onClick={() => setPendingRemoval(item)}
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

      <ConfirmDialog
        open={pendingRemoval !== null}
        onOpenChange={(open) => {
          if (!open) setPendingRemoval(null)
        }}
        title={t('treatments.removeItem')}
        confirmLabel={t('action.delete')}
        busy={deleteItem.isPending}
        onConfirm={() => {
          if (pendingRemoval) deleteItem.mutate({ id: pendingRemoval.id })
        }}
      >
        <p>{t('treatments.removeItemConfirm', { name: pendingRemoval?.name ?? '' })}</p>
        {/* A drug line holds a dispense; removing it books the quantity back. Worth saying, so
            the vet is not left wondering what happened to the stock. */}
        {pendingRemoval?.lots.length ? (
          <p className="mt-2 text-ink-faint">{t('treatments.removeItemReturnsStock')}</p>
        ) : null}
      </ConfirmDialog>
    </div>
  )
}
