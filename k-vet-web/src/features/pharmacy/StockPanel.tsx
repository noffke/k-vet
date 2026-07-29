import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { PackagePlus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  getListLotsQueryKey,
  useCreateStockIntake,
  useListLots,
} from '@/api/generated/endpoints'
import type { Drug, Packaging } from '@/api/generated/model'
import { DateInput } from '@/components/DateInput'
import { NumberInput } from '@/components/NumberInput'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { CheckboxField, TextField } from '@/components/ui/field'
import { todayIso } from '@/lib/format'
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
  const client = useQueryClient()

  const [intakeOpen, setIntakeOpen] = useState(false)
  const [showEmpty, setShowEmpty] = useState(false)
  const [arrivalDate, setArrivalDate] = useState<string | null>(todayIso())
  const [packages, setPackages] = useState<string | null>('1')
  const [batchNumber, setBatchNumber] = useState('')
  const [expirationDate, setExpirationDate] = useState<string | null>(null)

  const lots = useListLots({ drug_id: drug.id, empty: showEmpty || undefined })
  const createIntake = useCreateStockIntake({
    mutation: {
      onSuccess: async () => {
        setIntakeOpen(false)
        setBatchNumber('')
        setExpirationDate(null)
        await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        // The drug's derived stock changed with the delivery.
        await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drug.id) })
      },
    },
  })

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
            onClick={() => setIntakeOpen(true)}
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
              <span className="flex flex-wrap items-center justify-between gap-2">
                <span className="font-medium text-ink">
                  {lot.batch_number ?? `${t('pharmacy.lots')} ${lot.id}`}
                </span>
                <span className="numeric text-sm">
                  {formatQuantity(lot.remaining)} {lot.unit ?? ''}
                </span>
              </span>
              <span className="mt-0.5 flex flex-wrap gap-3 text-xs text-ink-faint">
                <span>
                  {t('field.arrivalDate')}: {date(lot.arrival_date)}
                </span>
                {lot.expiration_date ? (
                  <span>
                    {t('field.expirationDate')}: {date(lot.expiration_date)}
                  </span>
                ) : null}
                <span className="numeric">
                  {t('pharmacy.intake')}: {formatQuantity(lot.initial_quantity)}
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

      <Dialog
        open={intakeOpen}
        onOpenChange={setIntakeOpen}
        title={t('pharmacy.intake')}
        footer={
          <>
            <Button onClick={() => setIntakeOpen(false)}>{t('action.cancel')}</Button>
            <Button
              variant="primary"
              disabled={!original || !packages || createIntake.isPending}
              onClick={() =>
                original &&
                packages &&
                createIntake.mutate({
                  id: original.id,
                  data: {
                    arrival_date: arrivalDate,
                    packages_received: Number(packages),
                    batch_number: batchNumber || null,
                    expiration_date: expirationDate,
                  },
                })
              }
            >
              {t('action.add')}
            </Button>
          </>
        }
      >
        <div className="grid gap-3 sm:grid-cols-2">
          <DateInput label={t('field.arrivalDate')} value={arrivalDate} onChange={setArrivalDate} />
          <NumberInput
            label={t('field.packagesReceived')}
            value={packages}
            decimals={0}
            onChange={setPackages}
          />
          <TextField
            label={t('field.batchNumber')}
            value={batchNumber}
            onChange={(event) => setBatchNumber(event.target.value)}
          />
          <DateInput
            label={t('field.expirationDate')}
            value={expirationDate}
            onChange={setExpirationDate}
          />
          {original?.quantity && packages ? (
            <p className="numeric text-xs text-ink-faint sm:col-span-2">
              {t('pharmacy.intakeResult', {
                quantity: formatQuantity(String(Number(original.quantity) * Number(packages))),
                unit: original.unit ?? '',
              })}
            </p>
          ) : null}
        </div>
      </Dialog>
    </section>
  )
}
