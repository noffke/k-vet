import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
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
import { toWire } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { useGoBack } from '@/lib/trail'

/** What the vet typed last, which is what is booked: a count, or a change. */
type Entry = { kind: 'count' | 'change'; value: string | null }

/**
 * A correction on one Charge, two ways: a stocktake — what is actually on the shelf — or a
 * change — one broken bottle off, a pack found again on (issues.md, 2026-11-18). The two
 * fields follow each other; the one typed in last is what is booked, so a count stays a count
 * and a change a change even if a dispense lands meanwhile.
 *
 * The difference is booked as a movement of its own rather than by overwriting the stock —
 * the ledger is append-only, so a miscount stays visible as a correction (FR-016).
 */
export function StockCorrectionPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/lots/$id/correction' })
  const lotId = Number(id)
  const client = useQueryClient()

  const { quantity } = useLocaleFormat()
  const lot = useGetLot(lotId)
  const reasons = useListCorrectionReasons()
  // The page opens on the derived stock, so the vet corrects a number rather than typing one.
  const [entry, setEntry] = useState<Entry | null>(null)
  const [reason, setReason] = useState('')
  const remaining = lot.data?.remaining ?? null
  // Each field shows what the other one means: count = stock + change.
  const counted =
    entry === null
      ? remaining
      : entry.kind === 'count'
        ? entry.value
        : entry.value === null || remaining === null
          ? null
          : toWire(Number(remaining) + Number(entry.value))
  const change =
    entry === null
      ? null
      : entry.kind === 'change'
        ? entry.value
        : entry.value === null || remaining === null
          ? null
          : toWire(Number(entry.value) - Number(remaining))

  // Back where the vet came from — the appointment with its animal's tab — on success and on
  // cancel alike, rather than piling another entry onto the history.
  const back = useGoBack({ to: '/lots/$id', params: { id: String(lotId) } })
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

  // The server names the field and the reason; show that under the field it belongs to,
  // rather than assuming which it was.
  const failure: unknown = correct.error
  const errorOn = (field: 'new_remaining' | 'delta') =>
    failure instanceof ApiError ? failure.errorFor(field) : undefined
  const countError = errorOn('new_remaining')
  const changeError = errorOn('delta')
  const generic = correct.isError && !countError && !changeError
  // One −/+ press is one pack: a broken bottle is one press, not its millilitres typed.
  const packSize = record.packaging_quantity ? Number(record.packaging_quantity) : undefined

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

      {/* Opened by its address for a consumable "Ohne Bestand": no stock, no correction. */}
      {record.drug_without_stock ? (
        <p className="mt-3 text-sm text-ink-soft">{t('pharmacy.noCorrectionWithoutStock')}</p>
      ) : (
        <>
          <p className="mt-3 text-sm text-ink-soft">{t('pharmacy.correctionHint')}</p>
          <p className="mt-1 text-sm text-ink">
            {t('pharmacy.currentStock', {
              stock: `${quantity(record.remaining)} ${record.unit ?? ''}`.trim(),
            })}
          </p>

          <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
            <NumberInput
              label={t('pharmacy.remaining')}
              value={counted}
              unit={record.unit ?? undefined}
              // A count of what is on the shelf cannot be below zero (issues.md 8).
              min={0}
              // Every failure used to read "must not be 0", whatever it actually was — including
              // the refusal of a negative count, which then looked like the wrong complaint.
              error={countError ? t(countError) : undefined}
              onChange={(value) => setEntry({ kind: 'count', value })}
            />
            <NumberInput
              label={t('pharmacy.change')}
              value={change}
              unit={record.unit ?? undefined}
              step={packSize}
              hint={t('pharmacy.changeHint')}
              error={changeError ? t(changeError) : generic ? t('error.generic') : undefined}
              onChange={(value) => setEntry({ kind: 'change', value })}
            />
            {/* An editable select: previous reasons, alphabetical, or new free text. */}
            <TextField
              label={t('field.reason')}
              list="correction-reasons"
              value={reason}
              wrapperClassName="sm:col-span-2"
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
              disabled={entry === null || entry.value === null || correct.isPending}
              onClick={() =>
                entry !== null &&
                entry.value !== null &&
                correct.mutate({
                  id: lotId,
                  data:
                    entry.kind === 'change'
                      ? { delta: entry.value, reason: reason || null }
                      : { new_remaining: entry.value, reason: reason || null },
                })
              }
            >
              <SlidersHorizontal className="size-4" />
              {t('action.confirm')}
            </Button>
          </div>
        </>
      )}
    </div>
  )
}
