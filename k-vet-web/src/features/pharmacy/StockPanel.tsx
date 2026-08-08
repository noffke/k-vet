import { useNavigate } from '@tanstack/react-router'
import { PackagePlus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useListLots } from '@/api/generated/endpoints'
import type { Drug, Packaging } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

interface StockPanelProps {
  drug: Drug
  original: Packaging | undefined
}

/**
 * Stock of a drug: what arrived, what is left, and the way in — a delivery becomes a lot
 * whose initial quantity is snapshotted from the packaging (FR-015).
 */
export function StockPanel({ drug, original }: StockPanelProps) {
  const { t } = useTranslation()
  const { quantity: formatQuantity, date } = useLocaleFormat()
  const navigate = useNavigate()

  const [showEmpty, setShowEmpty] = useState(false)

  const lots = useListLots({ drug_id: drug.id, empty: showEmpty || undefined })

  return (
    <section className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-base">
          {t('pharmacy.stock')}
          <span className="numeric ml-2 text-sm font-normal text-ink-soft">
            {formatQuantity(drug.in_stock)} {original?.unit ?? ''}
          </span>
        </h2>
        <div className="flex items-center gap-3">
          <CheckboxField
            label={t('pharmacy.showEmptyLots')}
            checked={showEmpty}
            onChange={(event) => setShowEmpty(event.target.checked)}
          />
          <Button
            variant="accent"
            size="small"
            disabled={!original || original.draft}
            title={original ? undefined : t('pharmacy.original')}
            onClick={() =>
              void navigate({ to: '/pharmacy/$id/intake', params: { id: String(drug.id) } })
            }
          >
            <PackagePlus className="size-4" />
            {t('pharmacy.intake')}
          </Button>
        </div>
      </div>

      <ul className="mt-3 flex flex-col gap-2">
        {(lots.data ?? []).map((lot) => (
          <li key={lot.id}>
            <button
              type="button"
              onClick={() => void navigate({ to: '/lots/$id', params: { id: String(lot.id) } })}
              className="w-full rounded-card border border-line bg-surface px-3 py-2.5 text-left hover:bg-cream-soft"
            >
              {/* What arrived, not what it adds up to: the vet ordered packages. The
                  expiry sits opposite, because that is what decides which lot is used. */}
              <span className="flex flex-wrap items-center justify-between gap-2">
                <span className="numeric font-medium text-ink">
                  {lot.packaging_quantity
                    ? t('pharmacy.packageCount', {
                        count: lot.packages_received,
                        size: `${formatQuantity(lot.packaging_quantity)} ${lot.unit ?? ''}`.trim(),
                      })
                    : t('pharmacy.packageCountOnly', { count: lot.packages_received })}
                </span>
                {lot.expiration_date ? (
                  <span className="numeric text-sm">
                    {t('pharmacy.expiryShort')} {date(lot.expiration_date)}
                  </span>
                ) : null}
              </span>
              <span className="mt-0.5 flex flex-wrap gap-3 text-xs text-ink-faint">
                {lot.batch_number ? (
                  <span className="numeric">
                    {t('pharmacy.batchShort')} {lot.batch_number}
                  </span>
                ) : null}
                <span className="numeric">
                  {t('pharmacy.arrivalShort')} {date(lot.arrival_date)}
                </span>
                <span className="numeric">
                  {t('pharmacy.remaining')}: {formatQuantity(lot.remaining)} {lot.unit ?? ''}
                </span>
              </span>
            </button>
          </li>
        ))}
        {(lots.data ?? []).length === 0 ? (
          <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
            {t('list.empty')}
          </li>
        ) : null}
      </ul>
    </section>
  )
}
