import { useQueryClient } from '@tanstack/react-query'
import { Boxes, Package, RotateCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  getListLotsQueryKey,
  getListPackagingsQueryKey,
  useCreatePackaging,
  usePatchPackaging,
} from '@/api/generated/endpoints'
import type { AddressBookEntry, Drug, Packaging } from '@/api/generated/model'
import { NumberInput } from '@/components/NumberInput'
import { IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { SelectField, TextField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

interface PackagingEditorProps {
  drug: Drug
  packagings: Packaging[]
  suppliers: AddressBookEntry[]
}

/**
 * The packagings of a drug: one original (bought and stocked) and any number of subsets.
 *
 * The sales price is computed per AMPreisV from the purchase price and the VAT rate; the
 * computed value stays visible next to a manual override so the vet can see what the law
 * would charge (FR-014).
 */
export function PackagingEditor({ drug, packagings, suppliers }: PackagingEditorProps) {
  const { t } = useTranslation()
  const { money, quantity: formatQuantity } = useLocaleFormat()
  const client = useQueryClient()

  const refresh = async () => {
    await client.invalidateQueries({ queryKey: getListPackagingsQueryKey(drug.id) })
    await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
  }
  const patchPackaging = usePatchPackaging({ mutation: { onSuccess: refresh } })
  const createPackaging = useCreatePackaging({ mutation: { onSuccess: refresh } })

  const hasOriginal = packagings.some((packaging) => packaging.kind === 'original')

  return (
    <section className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-base">{t('pharmacy.packagings')}</h2>
        <div className="flex gap-2">
          <Button
            size="small"
            disabled={hasOriginal || drug.draft}
            title={hasOriginal ? t('packaging.originalAlreadyExists') : undefined}
            onClick={() => createPackaging.mutate({ id: drug.id, data: { kind: 'original' } })}
          >
            <Package className="size-4" />
            {t('pharmacy.original')}
          </Button>
          <Button
            size="small"
            disabled={!hasOriginal}
            onClick={() => createPackaging.mutate({ id: drug.id, data: { kind: 'subset' } })}
          >
            <Boxes className="size-4" />
            {t('pharmacy.subset')}
          </Button>
        </div>
      </div>

      <ul className="mt-3 flex flex-col gap-3">
        {packagings.map((packaging) => {
          const isOriginal = packaging.kind === 'original'
          const patch = (data: Parameters<typeof patchPackaging.mutate>[0]['data']) =>
            patchPackaging.mutate({ id: packaging.id, data })

          return (
            <li key={packaging.id} className="rounded-card border border-line bg-surface p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="flex items-center gap-2 text-sm font-semibold text-ink">
                  {isOriginal ? (
                    <Package className="size-4 text-rust" aria-hidden />
                  ) : (
                    <Boxes className="size-4 text-ink-soft" aria-hidden />
                  )}
                  {isOriginal ? t('pharmacy.original') : t('pharmacy.subset')}
                  {packaging.quantity ? (
                    <span className="numeric text-ink-soft">
                      {formatQuantity(packaging.quantity)} {packaging.unit ?? ''}
                    </span>
                  ) : null}
                </span>
                <IncompleteBadge missing={packaging.missing_fields} />
              </div>

              <div className="mt-2 grid gap-3 sm:grid-cols-4">
                <TextField
                  label={t('field.unit')}
                  defaultValue={packaging.unit ?? ''}
                  onBlur={(event) =>
                    event.target.value !== (packaging.unit ?? '') &&
                    patch({ unit: event.target.value || null })
                  }
                />
                <NumberInput
                  label={t('field.quantity')}
                  value={packaging.quantity ?? null}
                  onChange={(value) => value && patch({ quantity: value })}
                />
                <NumberInput
                  label={t('field.listPrice')}
                  value={packaging.list_price_net ?? null}
                  disabled={!isOriginal}
                  hint={isOriginal ? undefined : t('pharmacy.subsetPriceDerived')}
                  onChange={(value) => value && isOriginal && patch({ list_price_net: value })}
                />
                <NumberInput
                  label={t('field.salesPrice')}
                  value={packaging.sales_price_gross ?? null}
                  onChange={(value) => value && patch({ sales_price_gross: value })}
                />

                {isOriginal ? (
                  <SelectField
                    label={t('field.supplier')}
                    defaultValue={packaging.supplier_id ?? ''}
                    wrapperClassName="sm:col-span-2"
                    onChange={(event) =>
                      patch({
                        supplier_id: event.target.value ? Number(event.target.value) : null,
                      })
                    }
                  >
                    <option value="">—</option>
                    {suppliers.map((supplier) => (
                      <option key={supplier.id} value={supplier.id}>
                        {supplier.name ?? ''}
                      </option>
                    ))}
                  </SelectField>
                ) : null}

                <p className="self-end text-xs text-ink-faint sm:col-span-2">
                  {packaging.price_overridden ? (
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="font-medium text-rust">{t('pharmacy.priceOverride')}</span>
                      <span className="numeric">
                        {t('field.salesPrice')}: {money(packaging.computed_price_gross)}
                      </span>
                      <Button
                        size="small"
                        variant="ghost"
                        onClick={() => patch({ sales_price_gross: null })}
                      >
                        <RotateCcw className="size-3.5" />
                        {t('pharmacy.resetPrice')}
                      </Button>
                    </span>
                  ) : (
                    <span className="numeric">
                      {t('pharmacy.computedPerAmpreisv')}: {money(packaging.computed_price_gross)}
                    </span>
                  )}
                </p>
              </div>
            </li>
          )
        })}
        {packagings.length === 0 ? (
          <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
            {t('list.empty')}
          </li>
        ) : null}
      </ul>
    </section>
  )
}
