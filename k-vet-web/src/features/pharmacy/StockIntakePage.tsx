import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { PackagePlus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  getListLotsQueryKey,
  useCreateStockIntake,
  useGetDrug,
  useListPackagings,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { DateInput } from '@/components/DateInput'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'
import { todayIso } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'

/**
 * A delivery: what arrived, when, in how many packages, and until when it keeps.
 *
 * A page rather than a dialog — it is a record of goods received, entered while the box is
 * still in hand, and it opens fresh every time instead of holding the last delivery's count.
 */
export function StockIntakePage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/pharmacy/$id/intake' })
  const drugId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()
  const { quantity: formatQuantity } = useLocaleFormat()

  const drug = useGetDrug(drugId)
  const packagings = useListPackagings(drugId)
  const original = (packagings.data ?? []).find((packaging) => packaging.kind === 'original')

  const [arrivalDate, setArrivalDate] = useState<string | null>(todayIso())
  const [packages, setPackages] = useState<string | null>('1')
  const [batchNumber, setBatchNumber] = useState('')
  const [expirationDate, setExpirationDate] = useState<string | null>(null)

  const back = () => navigate({ to: '/pharmacy/$id', params: { id: String(drugId) } })
  const createIntake = useCreateStockIntake({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        // The drug's derived stock changed with the delivery.
        await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drugId) })
        await back()
      },
    },
  })

  if (drug.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!drug.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = drug.data

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        back={
          <BackLink
            to="/pharmacy/$id"
            params={{ id: String(drugId) }}
            label={record.name ?? t('pharmacy.title')}
          />
        }
        title={t('pharmacy.intake')}
      />

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <DateInput label={t('field.arrivalDate')} value={arrivalDate} onChange={setArrivalDate} />
        <NumberInput
          label={t('field.packagesReceived')}
          value={packages}
          decimals={0}
          step={1}
          min={1}
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
      </section>

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
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
          <PackagePlus className="size-4" />
          {t('action.add')}
        </Button>
      </div>
    </div>
  )
}
