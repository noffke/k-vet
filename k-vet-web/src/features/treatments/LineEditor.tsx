import { useQueryClient } from '@tanstack/react-query'
import {
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  FileText,
  Pill,
  Stethoscope,
  Trash2,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetTreatmentQueryKey,
  getListTreatmentItemsQueryKey,
  useApplyTemplate,
  useCreateTreatmentItem,
  useDeleteTreatmentItem,
  useListTemplates,
  useMoveTreatmentItem,
  usePatchTreatmentItem,
} from '@/api/generated/endpoints'
import type { MoveDirection, PickerItem, Treatment, TreatmentItem } from '@/api/generated/model'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useLocaleFormat } from '@/lib/locale'

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
  const { money, quantity: formatQuantity, percent } = useLocaleFormat()
  const client = useQueryClient()
  const [templateOpen, setTemplateOpen] = useState(false)

  const invalidate = async () => {
    await client.invalidateQueries({ queryKey: getListTreatmentItemsQueryKey(treatment.id) })
    await client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatment.id) })
  }
  const mutation = { onSuccess: invalidate }

  const addItem = useCreateTreatmentItem({ mutation })
  const patchItem = usePatchTreatmentItem({ mutation })
  const moveItem = useMoveTreatmentItem({ mutation })
  const deleteItem = useDeleteTreatmentItem({ mutation })
  const templates = useListTemplates(undefined, { query: { enabled: templateOpen } })
  const applyTemplate = useApplyTemplate({
    mutation: {
      onSuccess: async () => {
        setTemplateOpen(false)
        await invalidate()
      },
    },
  })

  const pick = (item: PickerItem) => {
    addItem.mutate({
      id: treatment.id,
      data:
        item.kind === 'drug_packaging'
          ? { kind: 'drug_packaging', drug_packaging_id: item.id, quantity: '1' }
          : { kind: 'service', service_id: item.id, quantity: '1' },
    })
  }

  const move = (itemId: number, direction: MoveDirection) =>
    moveItem.mutate({ id: itemId, data: { direction } })

  return (
    <section className="mt-6">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-base">{t('treatments.items')}</h2>
        <span className="numeric text-sm font-semibold text-ink">
          {t('treatments.total')}: {money(treatment.total_gross)}
        </span>
      </div>

      {readOnly ? null : (
        <>
          <div className="mt-3">
            <ItemPicker onPick={pick} disabled={addItem.isPending} />
          </div>
          <Button size="small" className="mt-2" onClick={() => setTemplateOpen(true)}>
            <FileText className="size-4" />
            {t('treatments.applyTemplate')}
          </Button>
        </>
      )}

      <ul className="mt-3 flex flex-col gap-2">
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
                  {item.got_number ? `GOT ${item.got_number} · ` : ''}
                  {percent(item.vat_percent)} {t('field.vat')}
                  {item.patient_id
                    ? ` · ${treatment.patients.find((patient) => patient.patient_id === item.patient_id)?.name ?? ''}`
                    : ''}
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
              {treatment.patients.length > 1 ? (
                <label className="flex flex-col gap-1">
                  <span className="text-xs font-semibold text-ink-soft">{t('field.patient')}</span>
                  <select
                    value={item.patient_id ?? ''}
                    disabled={readOnly}
                    onChange={(event) =>
                      patchItem.mutate({
                        id: item.id,
                        data: {
                          patient_id: event.target.value ? Number(event.target.value) : null,
                        },
                      })
                    }
                    className="rounded-control border border-line-strong bg-surface px-2 py-2 min-h-11 sm:min-h-9"
                  >
                    {item.kind === 'service' ? <option value="">—</option> : null}
                    {treatment.patients.map((patient) => (
                      <option key={patient.patient_id} value={patient.patient_id}>
                        {patient.name}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
            </div>

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
          <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
            {t('list.empty')}
          </li>
        ) : null}
      </ul>

      <Dialog
        open={templateOpen}
        onOpenChange={setTemplateOpen}
        title={t('treatments.applyTemplate')}
        footer={<Button onClick={() => setTemplateOpen(false)}>{t('action.cancel')}</Button>}
      >
        <ul className="flex flex-col gap-2">
          {(templates.data ?? [])
            .filter((template) => !template.draft)
            .map((template) => (
              <li key={template.id}>
                <button
                  type="button"
                  disabled={applyTemplate.isPending}
                  onClick={() =>
                    applyTemplate.mutate({
                      id: treatment.id,
                      data: { template_id: template.id },
                    })
                  }
                  className="w-full rounded-card border border-line bg-surface px-3 py-2.5 text-left hover:bg-cream-soft"
                >
                  <span className="font-medium text-ink">{template.name}</span>
                  <span className="ml-2 text-sm text-ink-soft">
                    {template.item_count} {t('templates.items')}
                  </span>
                </button>
              </li>
            ))}
          {(templates.data ?? []).filter((template) => !template.draft).length === 0 ? (
            <li className="text-sm text-ink-faint">{t('list.empty')}</li>
          ) : null}
        </ul>
      </Dialog>
    </section>
  )
}
