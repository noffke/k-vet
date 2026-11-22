import { useQueryClient } from '@tanstack/react-query'
import {
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Pill,
  RefreshCw,
  Stethoscope,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  getGetDashboardQueryKey,
  getGetTreatmentQueryKey,
  getListAppointmentsQueryKey,
  getListTreatmentItemsQueryKey,
  getPickerItemsQueryKey,
  useApplyTemplate,
  useBulkDeleteTreatmentItems,
  useCreateTreatmentItem,
  useDeleteTreatmentItem,
  useMoveTreatmentItem,
  usePatchTreatmentItem,
  useRecomputeTreatmentItemKm,
} from '@/api/generated/endpoints'
import type {
  MoveDirection,
  PatchTreatmentItem,
  PickerItem,
  Treatment,
  TreatmentItem,
} from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { PositionList, PositionRow } from '@/components/PositionRow'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { ValueCell } from '@/components/ValueCell'
import { ApplyQuote } from '@/features/quote/ApplyQuoteDialog'
import { useOperatorConfig } from '@/lib/config'
import { dispenseBreakdown, dispenseRuleText, refusalText } from '@/lib/dispense'
import { useLocaleFormat } from '@/lib/locale'
import { refusalKey } from '@/lib/refusal'
import { cn } from '@/lib/utils'
import { useKmToast } from './kmToast'

/** How long the undo stays on screen. Sonner's 4 s default is too quick to read and decide. */
const UNDO_MS = 10_000

/** A position from the GOT itself — not a self-defined service charged by analogy (§ 8). */
function gotPosition(item: TreatmentItem): boolean {
  return item.got_number != null && !item.got_analogous
}

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
  /**
   * The line just picked, on this visit — kept by the treatment section, since a "pro Termin"
   * service picked in an animal's tab lands in the visit's group. Its values stay open.
   */
  newestId?: number | null
  /** Told the id of each line picked here. */
  onAdded?: ((id: number) => void) | undefined
  /** The search field of the group that has one, for Enter to return to. */
  pickerRef?: { current: HTMLInputElement | null } | undefined
  /** Enter in the newest line's value: back to the search, for the next position. */
  onReturnToSearch?: (() => void) | undefined
}

/** A value of a position's row that is being typed into. */
type Cell = 'quantity' | 'factor' | 'km'

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
  newestId = null,
  onAdded,
  pickerRef,
  onReturnToSearch,
}: PositionGroupProps) {
  const { t } = useTranslation()
  const { money, unitPrice, quantity: formatQuantity, percent } = useLocaleFormat()
  const { maps_enabled: mapsEnabled } = useOperatorConfig()
  const client = useQueryClient()

  // Lines that print differently from the invoice PDF on file (migration 0034).
  const changedSincePdf = new Set(treatment.pdf_changes?.changed_items ?? [])

  const invalidate = async () => {
    await client.invalidateQueries({ queryKey: getListTreatmentItemsQueryKey(treatment.id) })
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
    // The lists and the dashboard say whether a PDF still matches; read afresh when shown.
    await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
    await client.invalidateQueries({ queryKey: getGetDashboardQueryKey() })
    // A drug line takes stock off the shelf or puts it back: the picker's figures move with it.
    await client.invalidateQueries({ queryKey: getPickerItemsQueryKey() })
  }
  /**
   * A refused write is said, not swallowed: a drug the lots cannot cover would otherwise just not
   * appear (issues.md). Reloaded as well, so a quantity field shows what was kept.
   */
  const refused = (error: unknown) => {
    toast.error(t(refusalKey(error)))
    void invalidate()
  }
  const mutation = { onSuccess: invalidate, onError: refused }

  // The line is the newest before the list is read again, so it comes in with its values open
  // (a call's own `onSuccess` runs only after this one's refetch).
  const addItem = useCreateTreatmentItem({
    mutation: {
      onSuccess: (created) => {
        onAdded?.(created.id)
        return invalidate()
      },
      onError: refused,
    },
  })
  // A quantity off a drug's steps is refused with the steps themselves: "in Schritten von 10 ml".
  const patchItem = usePatchTreatmentItem({
    mutation: {
      onSuccess: invalidate,
      onError: (error, variables) => {
        const line = items.find((item) => item.id === variables.id)
        toast.error(
          refusalText(refusalKey(error), line?.dispense?.rule, line?.unit, t, formatQuantity),
        )
        void invalidate()
      },
    },
  })
  const moveItem = useMoveTreatmentItem({ mutation })
  const deleteItem = useDeleteTreatmentItem({ mutation })
  const recomputeKm = useRecomputeTreatmentItemKm({ mutation })

  const kmToast = useKmToast()
  /**
   * The line a removal is being confirmed for. Deleting sits in a row of five icon buttons,
   * next to "move down" — on a phone that is one thumb's width from a position the vet meant
   * to reorder, and a removed drug line takes its dispense with it.
   */
  const [pendingRemoval, setPendingRemoval] = useState<TreatmentItem | null>(null)
  /** A value opened with a tap, until the focus leaves it (the newest line's stay open). */
  const [editing, setEditing] = useState<{ id: number; cell: Cell } | null>(null)
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
            ? // No quantity: the drug starts at what it allows — one unit, else one step.
              { kind: 'drug_packaging', drug_packaging_id: item.id, patient_treatment_id }
            : { kind: 'service', service_id: item.id, quantity: '1', patient_treatment_id },
      },
      {
        onSuccess: (created) => {
          // A Wegegeld line's kilometres say more than "added" would.
          if (kmToast(created.km_suggestion, item.name)) return
          toast(
            t(elsewhere ? 'treatments.itemAddedPerVisit' : 'treatments.itemAdded', {
              name: item.name,
            }),
          )
        },
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
          inputRef={pickerRef}
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
        {/* A Preisauskunft the customer was given goes in whole, like a group. */}
        {patientTreatmentId === null ? null : (
          <ApplyQuote
            treatmentId={treatment.id}
            customerId={treatment.customer_id}
            patientTreatmentId={patientTreatmentId}
            disabled={addItem.isPending}
            onApplied={(applied, name) => {
              const before = new Set(
                (
                  client.getQueryData<TreatmentItem[]>(
                    getListTreatmentItemsQueryKey(treatment.id),
                  ) ?? []
                ).map((item) => item.id),
              )
              void invalidate()
              offerUndo(applied, before, name)
            }}
          />
        )}
      </div>
    )

  return (
    <div className="mt-4">
      <h3 className="text-sm font-semibold text-ink">{heading}</h3>
      {patientTreatmentId === null && !readOnly && showPicker ? (
        <p className="mt-0.5 text-xs text-ink-faint">{t('treatments.withoutPatientHint')}</p>
      ) : null}

      {items.length === 0 ? (
        <p className="mt-2 rounded-card border border-dashed border-line-strong px-4 py-4 text-center text-sm text-ink-faint">
          {t('list.empty')}
        </p>
      ) : (
        <PositionList label={typeof heading === 'string' ? heading : t('treatments.items')}>
          {items.map((item, index) => {
            const newest = item.id === newestId && !readOnly
            const isOpen = (cell: Cell) =>
              newest || (editing?.id === item.id && editing.cell === cell)
            const close = (cell: Cell) => () =>
              setEditing((current) =>
                current?.id === item.id && current.cell === cell ? null : current,
              )
            const open = (cell: Cell) => () => setEditing({ id: item.id, cell })
            const quantityCell = item.travel_expenses ? (
              <ValueCell
                label={t('field.km')}
                display={
                  item.km != null ? `${formatQuantity(item.km)} km` : t('treatments.kmMissingShort')
                }
                missing={item.km == null}
                value={item.km ?? null}
                unit="km"
                open={isOpen('km')}
                onOpen={open('km')}
                onClose={close('km')}
                autoFocus={newest}
                onEnter={newest ? onReturnToSearch : undefined}
                readOnly={readOnly}
                onChange={(value) =>
                  value && patchItem.mutate({ id: item.id, data: { km: value } })
                }
                action={
                  mapsEnabled
                    ? {
                        label: t('treatments.recomputeKm'),
                        icon: (
                          <RefreshCw
                            className={cn(
                              'size-4',
                              recomputeKm.isPending &&
                                recomputeKm.variables?.id === item.id &&
                                'animate-spin',
                            )}
                          />
                        ),
                        disabled: recomputeKm.isPending,
                        onClick: () =>
                          recomputeKm.mutate(
                            { id: item.id },
                            { onSuccess: (line) => kmToast(line.km_suggestion, item.name) },
                          ),
                      }
                    : undefined
                }
              />
            ) : (
              <ValueCell
                label={t('field.quantity')}
                display={`${formatQuantity(item.quantity)} ${
                  item.dispense ? (item.unit ?? '') : '×'
                }`.trim()}
                value={item.quantity}
                unit={item.dispense ? (item.unit ?? undefined) : undefined}
                open={isOpen('quantity')}
                onOpen={open('quantity')}
                onClose={close('quantity')}
                autoFocus={newest}
                onEnter={newest ? onReturnToSearch : undefined}
                readOnly={readOnly}
                onChange={(value) =>
                  value && patchItem.mutate({ id: item.id, data: { quantity: value } })
                }
              />
            )
            const factorCell =
              item.kind === 'service' ? (
                <ValueCell
                  label={t('field.factor')}
                  display={percent(item.factor)}
                  value={item.factor ?? null}
                  decimals={3}
                  unit="%"
                  open={isOpen('factor')}
                  onOpen={open('factor')}
                  onClose={close('factor')}
                  onEnter={newest ? onReturnToSearch : undefined}
                  readOnly={readOnly}
                  onChange={(value) =>
                    value && patchItem.mutate({ id: item.id, data: { factor: value } })
                  }
                />
              ) : null
            const breakdown = item.dispense
              ? dispenseBreakdown(item.dispense, item.unit, t, {
                  quantity: formatQuantity,
                  money,
                  unitPrice,
                })
              : null
            // "GOT 25 · 19 % USt." — said once: in the subline while a value is open, else in
            // the details.
            const meta = `${
              item.got_number ? `${item.got_analogous ? '§8' : 'GOT'} ${item.got_number} · ` : ''
            }${percent(item.vat_percent)} ${t('field.vat')}`
            const typing = isOpen('quantity') || isOpen('factor') || isOpen('km')
            const kmMissing = item.travel_expenses && item.km == null && !readOnly
            const changed = changedSincePdf.has(item.id)
            return (
              <PositionRow
                key={item.id}
                icon={
                  item.kind === 'drug_packaging' ? (
                    <Pill className="size-4 shrink-0 text-rust" aria-hidden />
                  ) : (
                    <Stethoscope className="size-4 shrink-0 text-ink-soft" aria-hidden />
                  )
                }
                name={item.name}
                quantity={quantityCell}
                factor={factorCell}
                amount={money(item.line_gross)}
                newest={newest}
                marker={changed}
                // While a value is typed: the steps, how it bills, and GOT and VAT — so opening
                // the details is never needed just to read them.
                hint={
                  typing ? (
                    <span className="numeric">
                      {[
                        item.dispense
                          ? dispenseRuleText(item.dispense.rule, item.unit, t, formatQuantity)
                          : null,
                        breakdown,
                        meta,
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                  ) : undefined
                }
                notice={
                  kmMissing || changed ? (
                    <>
                      {kmMissing ? (
                        <span className="font-medium text-danger">
                          {t('treatments.kmRequiredForInvoice')}
                        </span>
                      ) : null}
                      {changed ? (
                        <span className="font-semibold text-rust">{t('pdfStale.lineChanged')}</span>
                      ) : null}
                    </>
                  ) : undefined
                }
                removeLabel={t('action.delete')}
                onRemove={readOnly ? undefined : () => setPendingRemoval(item)}
                details={
                  <LineDetails
                    item={item}
                    index={index}
                    count={items.length}
                    treatment={treatment}
                    readOnly={readOnly}
                    breakdown={breakdown}
                    meta={typing ? null : meta}
                    onPatch={(data) => patchItem.mutate({ id: item.id, data })}
                    onMove={(direction) => move(item.id, direction)}
                  />
                }
              />
            )
          })}
        </PositionList>
      )}

      {/* Under the list: a picked position appears right above the search it came from,
          where the vet is looking, instead of off screen at the end of a long list. */}
      {pickerBlock}

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

interface LineDetailsProps {
  item: TreatmentItem
  index: number
  count: number
  treatment: Treatment
  readOnly: boolean
  /** What the invoice prints for the quantity, already composed. */
  breakdown: string | null
  /** "GOT 25 · 19 % USt." — `null` while the row's subline already says it, with the breakdown. */
  meta: string | null
  onPatch: (data: PatchTreatmentItem) => void
  onMove: (direction: MoveDirection) => void
}

/** Everything of a line that waits behind its name: rename, prices, animal, notes, order. */
function LineDetails({
  item,
  index,
  count,
  treatment,
  readOnly,
  breakdown,
  meta,
  onPatch,
  onMove,
}: LineDetailsProps) {
  const { t } = useTranslation()
  const { money, quantity: formatQuantity, currencySymbol } = useLocaleFormat()

  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-3 sm:grid-cols-3">
        <label className="flex flex-col gap-1 sm:col-span-3">
          <span className="text-xs font-semibold text-ink-soft">{t('field.name')}</span>
          <input
            defaultValue={item.name}
            disabled={readOnly}
            onBlur={(event) => {
              if (event.target.value !== item.name) onPatch({ name: event.target.value })
            }}
            className="min-h-11 rounded-control border border-line-strong bg-surface px-3 py-2 text-ink disabled:bg-sunken disabled:text-ink-faint sm:min-h-9"
          />
        </label>
        {/* A Wegegeld line shows its kilometres in the row; its count is here. */}
        {item.travel_expenses ? (
          <NumberInput
            label={t('field.quantity')}
            value={item.quantity}
            disabled={readOnly}
            onChange={(value) => value && onPatch({ quantity: value })}
          />
        ) : null}
        {/* A GOT position costs what the schedule says, net as the GOT publishes it; the
            factor adjusts it, the price stays (the server refuses one). */}
        {gotPosition(item) ? (
          // Not a field: nothing to type, so nothing greyed out either.
          <div className="flex flex-col gap-1">
            <span className="text-xs font-semibold text-ink-soft">{t('field.priceNet')}</span>
            <p className="flex min-h-11 flex-wrap items-center gap-x-2 text-ink sm:min-h-9">
              <span className="numeric">{money(item.price_net)}</span>
              <span className="text-ink-soft">
                · {t('field.priceGross')}:{' '}
                <span className="numeric">{money(item.price_gross)}</span>
              </span>
            </p>
            <p className="text-xs text-ink-faint">{t('treatments.gotPriceHint')}</p>
          </div>
        ) : (
          // Every other position is priced as the customer sees it: gross, after the factor,
          // with the net underneath. The server stores the net that comes closest.
          <NumberInput
            label={item.dispense ? t('dispense.packPrice') : t('field.priceGross')}
            value={item.price_gross}
            unit={currencySymbol}
            money
            hint={`${t('field.priceNet')}: ${money(item.unit_net)}`}
            disabled={readOnly}
            onChange={(value) => value && onPatch({ price_gross: value })}
          />
        )}
        {/* The rest's price per unit, when there is a rest: stored for the Teilmenge. */}
        {item.dispense?.remainder ? (
          <NumberInput
            label={t('dispense.subsetPrice', { unit: item.unit ?? '' })}
            value={item.dispense.remainder.price_gross}
            unit={currencySymbol}
            decimals={4}
            disabled={readOnly}
            onChange={(value) => value && onPatch({ subset_price_gross: value })}
          />
        ) : null}
        {item.travel_expenses ? (
          <NumberInput
            label={t('field.kmMultiplier')}
            value={item.km_multiplier ?? null}
            decimals={3}
            disabled={readOnly}
            hint={t('treatments.kmHint')}
            onChange={(value) => value && onPatch({ km_multiplier: value })}
          />
        ) : null}
        {/* Moving a line to the animal it belongs to, when it was entered under the wrong
            one. Never between an animal and the visit: that is the service's to say. */}
        {treatment.patients.length > 1 && item.patient_treatment_id !== null ? (
          <label className="flex flex-col gap-1">
            <span className="text-xs font-semibold text-ink-soft">{t('field.patient')}</span>
            <select
              value={item.patient_treatment_id}
              disabled={readOnly}
              onChange={(event) => onPatch({ patient_treatment_id: Number(event.target.value) })}
              className="min-h-11 rounded-control border border-line-strong bg-surface px-2 py-2 sm:min-h-9"
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
        <div>
          <CheckboxField
            label={t('treatments.redesignation')}
            defaultChecked={item.redesignation}
            disabled={readOnly}
            onChange={(event) => onPatch({ redesignation: event.target.checked })}
          />
          <p className="text-xs text-ink-faint">{t('treatments.redesignationHint')}</p>
        </div>
      ) : null}

      <div className="flex flex-wrap items-end justify-between gap-2">
        <div className="min-w-0 text-xs text-ink-faint">
          {/* GOT, VAT and what the invoice prints for the quantity — unless the row's subline
              is saying them right now. */}
          {meta ? <p>{meta}</p> : null}
          {meta && breakdown ? <p className="numeric text-ink-soft">{breakdown}</p> : null}
          {item.lots.length > 0 ? (
            <p>
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
        </div>
        {readOnly ? null : (
          <div className="flex gap-1">
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('action.moveTop')}
              disabled={index === 0}
              onClick={() => onMove('top')}
            >
              <ChevronsUp className="size-4" />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('action.moveUp')}
              disabled={index === 0}
              onClick={() => onMove('up')}
            >
              <ChevronUp className="size-4" />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('action.moveDown')}
              disabled={index === count - 1}
              onClick={() => onMove('down')}
            >
              <ChevronDown className="size-4" />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('action.moveBottom')}
              disabled={index === count - 1}
              onClick={() => onMove('bottom')}
            >
              <ChevronsDown className="size-4" />
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}
