import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { SlidersHorizontal } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  getGetDrugQueryKey,
  getGetLotQueryKey,
  getListDrugsQueryKey,
  useCreateCorrection,
  useGetLot,
  useListCorrectionReasons,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'

/**
 * A stocktake on one Charge: what is actually on the shelf, and why it differs.
 *
 * The difference is booked as a movement of its own rather than by overwriting the stock —
 * the ledger is append-only, so a miscount stays visible as a correction (FR-016).
 */
export function StockCorrectionPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/lots/$id/correction' })
  const lotId = Number(id)
  const navigate = useNavigate()
  const client = useQueryClient()

  const lot = useGetLot(lotId)
  const reasons = useListCorrectionReasons()
  // The page opens on the derived stock, so the vet corrects a number rather than typing one.
  const [newRemaining, setNewRemaining] = useState<string | null>(null)
  const [reason, setReason] = useState('')
  const counted = newRemaining ?? lot.data?.remaining ?? null

  const back = () => navigate({ to: '/lots/$id', params: { id: String(lotId) } })
  const correct = useCreateCorrection({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getGetLotQueryKey(lotId) })
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        if (lot.data) {
          await client.invalidateQueries({ queryKey: getGetDrugQueryKey(lot.data.drug_id) })
        }
        await back()
      },
    },
  })

  if (lot.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!lot.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = lot.data

  // The server names the field and the reason; show that rather than assuming which it was.
  const failure: unknown = correct.error
  const correctionError = correct.isError
    ? t(
        (failure instanceof ApiError ? failure.errorFor('new_remaining') : undefined) ??
          'error.generic',
      )
    : undefined

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        back={
          <BackLink
            to="/lots/$id"
            params={{ id: String(lotId) }}
            label={
              record.batch_number
                ? `${t('pharmacy.batchShort')} ${record.batch_number}`
                : `${t('pharmacy.lots')} ${record.id}`
            }
          />
        }
        title={t('pharmacy.correction')}
      />

      {/* The field opens on the derived stock, so what stands there is what the app thinks
          is on the shelf — no need to print it a second time. */}
      <p className="mt-3 text-sm text-ink-soft">{t('pharmacy.correctionHint')}</p>

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <NumberInput
          label={t('pharmacy.remaining')}
          value={counted}
          unit={record.unit ?? undefined}
          // A count of what is on the shelf cannot be below zero (issues.md 8).
          min={0}
          // Every failure used to read "must not be 0", whatever it actually was — including
          // the refusal of a negative count, which then looked like the wrong complaint.
          error={correctionError}
          onChange={setNewRemaining}
        />
        {/* An editable select: previous reasons, alphabetical, or new free text. */}
        <TextField
          label={t('field.reason')}
          list="correction-reasons"
          value={reason}
          onChange={(event) => setReason(event.target.value)}
        />
        <datalist id="correction-reasons">
          {(reasons.data ?? []).map((entry) => (
            <option key={entry} value={entry} />
          ))}
        </datalist>
      </section>

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={counted === null || correct.isPending}
          onClick={() =>
            counted !== null &&
            correct.mutate({
              id: lotId,
              data: { new_remaining: counted, reason: reason || null },
            })
          }
        >
          <SlidersHorizontal className="size-4" />
          {t('action.confirm')}
        </Button>
      </div>
    </div>
  )
}
