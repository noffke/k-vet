import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { Link2, PackagePlus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  addDrugCode,
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  getListLotsQueryKey,
  getListPackagingsQueryKey,
  readScan,
  useCreateStockIntake,
  useGetDrug,
  useListPackagings,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { ScanField } from '@/components/ScanField'
import { Button } from '@/components/ui/button'
import { useGoBack } from '@/lib/trail'
import { IntakeFields } from './IntakeFields'
import { canBook, freshIntake, intakeBody } from './intake'

/**
 * A delivery of one drug: what arrived, when, in how many packages, until when it keeps — and
 * whether the list price is still right.
 *
 * A page rather than a dialog — it is a record of goods received, entered while the box is
 * still in hand, and it opens fresh every time instead of holding the last delivery's count.
 * Scanning the pack's Data Matrix fills batch and expiry; a code the drug does not know yet
 * can be added to it from here.
 */
export function StockIntakePage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/pharmacy/$id/intake' })
  const drugId = Number(id)
  const client = useQueryClient()

  const drug = useGetDrug(drugId)
  const packagings = useListPackagings(drugId)
  const original = (packagings.data ?? []).find((packaging) => packaging.kind === 'original')

  const [draft, setDraft] = useState(() => freshIntake())
  const [scanProblem, setScanProblem] = useState<string | null>(null)
  // A code of this pack that no drug has yet: offered to be added to this one.
  const [unknownCode, setUnknownCode] = useState<string | null>(null)

  // Back where the vet came from — the appointment with its animal's tab — on success and on
  // cancel alike, rather than piling another entry onto the history.
  const back = useGoBack({ to: '/pharmacy/$id', params: { id: String(drugId) } })
  const createIntake = useCreateStockIntake({
    mutation: {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        // The drug's derived stock changed with the delivery — and a changed list price moved
        // the packagings' prices.
        await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drugId) })
        await client.invalidateQueries({ queryKey: getListPackagingsQueryKey(drugId) })
        await back()
      },
    },
  })

  const scan = async (raw: string) => {
    setScanProblem(null)
    setUnknownCode(null)
    try {
      const scanned = await readScan({ raw })
      if (scanned.drug && scanned.drug.id !== drugId) {
        setScanProblem(t('pharmacy.codeOfOther', { name: scanned.drug.name ?? '' }))
        return
      }
      if (!scanned.gtin && !scanned.batch && !scanned.expiry) {
        setScanProblem(t('code.invalid'))
        return
      }
      setDraft((current) => ({
        ...current,
        batch: scanned.batch ?? current.batch,
        expiry: scanned.expiry ?? current.expiry,
      }))
      if (scanned.gtin && !scanned.drug) setUnknownCode(raw)
    } catch {
      setScanProblem(t('error.generic'))
    }
  }

  const attachCode = async () => {
    if (!unknownCode) return
    try {
      client.setQueryData(
        getGetDrugQueryKey(drugId),
        await addDrugCode(drugId, { code: unknownCode }),
      )
      setUnknownCode(null)
    } catch {
      setScanProblem(t('error.generic'))
    }
  }

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
        eyebrow={record.name ?? undefined}
        title={t('pharmacy.intake')}
      />

      <section className="mt-5 flex flex-col gap-2 rounded-card border border-line bg-surface p-4">
        <ScanField
          label={t('intake.scan')}
          placeholder={t('intake.scanPlaceholder')}
          error={scanProblem ?? undefined}
          autoFocus
          onScan={(raw) => void scan(raw)}
        />
        {unknownCode ? (
          <div className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
            {t('intake.unknownCode')}
            <Button size="small" onClick={() => void attachCode()}>
              <Link2 className="size-4" />
              {t('intake.attachCode', { name: record.name ?? '' })}
            </Button>
          </div>
        ) : null}
      </section>

      {original ? (
        <section className="mt-4 rounded-card border border-line bg-surface p-4">
          <IntakeFields drug={record} original={original} value={draft} onChange={setDraft} />
        </section>
      ) : null}

      <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
        {!canBook(draft) && draft.price.state === 'open' ? (
          <p className="text-xs text-ink-faint">{t('intake.needsPrice')}</p>
        ) : null}
        <Button onClick={() => void back()}>{t('action.cancel')}</Button>
        <Button
          variant="primary"
          disabled={!original || !canBook(draft) || createIntake.isPending}
          onClick={() =>
            original && createIntake.mutate({ id: original.id, data: intakeBody(draft, original) })
          }
        >
          <PackagePlus className="size-4" />
          {t('action.add')}
        </Button>
      </div>
    </div>
  )
}
