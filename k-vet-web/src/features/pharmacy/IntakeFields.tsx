import { Check, PencilLine } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { previewPrice } from '@/api/generated/endpoints'
import type { Drug, Packaging, PricePreview } from '@/api/generated/model'
import { DateInput } from '@/components/DateInput'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'
import type { IntakeDraft, PriceCheck } from './intake'

interface IntakeFieldsProps {
  drug: Drug
  original: Packaging
  value: IntakeDraft
  onChange: (next: IntakeDraft) => void
}

/**
 * What a Wareneingang asks: arrival, packages, batch and expiry — the last two often filled from
 * a scanned Data Matrix — and the list price check. Shared by the single drug's Wareneingang and
 * the delivery page, so both ask the same.
 */
export function IntakeFields({ drug, original, value, onChange }: IntakeFieldsProps) {
  const { t } = useTranslation()
  const { quantity: formatQuantity } = useLocaleFormat()
  const set = (patch: Partial<IntakeDraft>) => onChange({ ...value, ...patch })

  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <DateInput
        label={t('field.arrivalDate')}
        value={value.arrivalDate}
        onChange={(arrivalDate) => set({ arrivalDate })}
      />
      <NumberInput
        label={t('field.packagesReceived')}
        value={value.packages}
        decimals={0}
        step={1}
        min={1}
        onChange={(packages) => set({ packages })}
      />
      <TextField
        label={t('field.batchNumber')}
        value={value.batch}
        onChange={(event) => set({ batch: event.target.value })}
      />
      {/* Packages print the expiry as month and year; that is the month's last day. */}
      <DateInput
        label={t('field.expirationDate')}
        value={value.expiry}
        onChange={(expiry) => set({ expiry })}
        monthEnd
      />
      {original.quantity && value.packages ? (
        <p className="numeric text-xs text-ink-faint sm:col-span-2">
          {t('pharmacy.intakeResult', {
            quantity: formatQuantity(String(Number(original.quantity) * Number(value.packages))),
            unit: original.unit ?? '',
          })}
        </p>
      ) : null}
      <ListPriceCheck
        drug={drug}
        original={original}
        value={value.price}
        onChange={(price) => set({ price })}
      />
    </div>
  )
}

/**
 * "Listenpreis 12,40 € — stimmt der noch?" A changed wholesaler price is a common error: it
 * goes unnoticed, and every sale after it is priced from the old one. So each Wareneingang asks,
 * and a new price shows what the sales price becomes before anything is booked. The price is
 * stored together with the lot.
 */
function ListPriceCheck({
  drug,
  original,
  value,
  onChange,
}: {
  drug: Drug
  original: Packaging
  value: PriceCheck
  onChange: (next: PriceCheck) => void
}) {
  const { t } = useTranslation()
  const { money, currencySymbol } = useLocaleFormat()
  const [preview, setPreview] = useState<PricePreview | null>(null)

  const changedTo = value.state === 'changed' ? value.value : null
  useEffect(() => {
    setPreview(null)
    if (!changedTo || Number(changedTo) <= 0 || !drug.vat_percent) return
    let current = true
    void previewPrice({
      kind: 'original',
      list_price_net: changedTo,
      vat_percent: drug.vat_percent,
      human_drug: drug.human_drug,
    })
      .then((result) => {
        if (current) setPreview(result)
      })
      .catch(() => undefined)
    return () => {
      current = false
    }
  }, [changedTo, drug.vat_percent, drug.human_drug])

  const listed = money(original.list_price_net)
  return (
    <fieldset className="rounded-card border border-line bg-cream-soft/40 p-3 sm:col-span-2">
      <legend className="eyebrow px-1">{t('intake.priceCheck')}</legend>
      {value.state === 'earlier' ? (
        <p className="flex items-center gap-2 text-sm text-ink-soft">
          <Check className="size-4 text-sage-deep" aria-hidden />
          {t('intake.priceConfirmedEarlier', { price: listed })}
        </p>
      ) : value.state === 'same' ? (
        <p className="flex flex-wrap items-center gap-2 text-sm text-ink">
          <Check className="size-4 text-sage-deep" aria-hidden />
          {t('intake.priceConfirmed', { price: listed })}
          <Button size="small" variant="ghost" onClick={() => onChange({ state: 'open' })}>
            {t('action.change')}
          </Button>
        </p>
      ) : value.state === 'changed' ? (
        <div className="flex flex-col gap-2">
          <NumberInput
            label={t('intake.newListPrice', { price: listed })}
            value={value.value}
            unit={currencySymbol}
            money
            onChange={(next) => onChange({ state: 'changed', value: next })}
          />
          {preview ? (
            <p className="numeric text-sm text-ink-soft">
              {original.price_overridden
                ? t('intake.salesPriceManual', { price: money(original.sales_price_gross) })
                : t('intake.salesPriceBecomes', {
                    price: money(preview.gross),
                    before: money(original.sales_price_gross),
                  })}
            </p>
          ) : null}
          <div>
            <Button size="small" variant="ghost" onClick={() => onChange({ state: 'open' })}>
              {t('action.cancel')}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <p className="text-sm text-ink">
            {t('intake.priceQuestion', { price: listed })}
            <span className="block text-xs text-ink-faint">{t('field.listPriceHint')}</span>
          </p>
          <div className="flex flex-wrap gap-2">
            <Button size="small" variant="primary" onClick={() => onChange({ state: 'same' })}>
              <Check className="size-4" />
              {t('intake.priceSame')}
            </Button>
            <Button size="small" onClick={() => onChange({ state: 'changed', value: null })}>
              <PencilLine className="size-4" />
              {t('intake.priceChanged')}
            </Button>
          </div>
        </div>
      )}
    </fieldset>
  )
}
