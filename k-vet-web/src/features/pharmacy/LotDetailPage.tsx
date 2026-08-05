import { useQueryClient } from '@tanstack/react-query'
import { Link, useParams } from '@tanstack/react-router'
import { ArrowDownRight, ArrowUpRight, SlidersHorizontal } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
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
import { Dialog } from '@/components/ui/dialog'
import { TextField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/**
 * One lot's life: what arrived, every movement since, and where each dispense went.
 *
 * This is the batch-traceability view (FR-017, SC-004) — from a batch number to the
 * animals and customers that received it, and to the invoice that billed it.
 */
export function LotDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/lots/$id' })
  const lotId = Number(id)
  const client = useQueryClient()
  const { quantity: formatQuantity, date, dateTime } = useLocaleFormat()

  const lot = useGetLot(lotId)
  const reasons = useListCorrectionReasons()
  const [correctionOpen, setCorrectionOpen] = useState(false)
  const [newRemaining, setNewRemaining] = useState<string | null>(null)
  const [reason, setReason] = useState('')

  const correct = useCreateCorrection({
    mutation: {
      onSuccess: async () => {
        setCorrectionOpen(false)
        setReason('')
        await client.invalidateQueries({ queryKey: getGetLotQueryKey(lotId) })
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        if (lot.data) {
          await client.invalidateQueries({ queryKey: getGetDrugQueryKey(lot.data.drug_id) })
        }
      },
    },
  })

  if (lot.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!lot.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = lot.data

  /** The correction dialog opens pre-filled with the derived stock (FR-016). */
  const openCorrection = () => {
    setNewRemaining(record.remaining)
    setCorrectionOpen(true)
  }

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={
          <BackLink
            to="/pharmacy/$id"
            params={{ id: String(record.drug_id) }}
            label={record.drug_name ?? t('pharmacy.title')}
          />
        }
        title={record.batch_number ?? `${t('pharmacy.lots')} ${record.id}`}
        actions={
          <Button variant="accent" onClick={openCorrection}>
            <SlidersHorizontal className="size-4" />
            {t('pharmacy.correction')}
          </Button>
        }
      />

      <dl className="mt-5 grid grid-cols-2 gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-4">
        <Fact label={t('pharmacy.remaining')}>
          <span className="numeric text-lg font-semibold text-ink">
            {formatQuantity(record.remaining)} {record.unit ?? ''}
          </span>
        </Fact>
        <Fact label={t('pharmacy.intake')}>
          <span className="numeric">
            {formatQuantity(record.initial_quantity)} {record.unit ?? ''}
          </span>
        </Fact>
        <Fact label={t('field.arrivalDate')}>{date(record.arrival_date)}</Fact>
        <Fact label={t('field.expirationDate')}>
          {record.expiration_date ? date(record.expiration_date) : '—'}
        </Fact>
      </dl>

      <section className="mt-6">
        <h2 className="text-base">{t('pharmacy.movements')}</h2>
        <ul className="mt-2 flex flex-col gap-2">
          {record.movements.map((movement) => {
            const outgoing = Number(movement.quantity) < 0
            return (
              <li
                key={movement.id}
                className="rounded-card border border-line bg-surface px-3 py-2.5"
              >
                <div className="flex items-start justify-between gap-3">
                  <span className="flex items-center gap-2 text-sm">
                    {outgoing ? (
                      <ArrowDownRight className="size-4 shrink-0 text-danger" aria-hidden />
                    ) : (
                      <ArrowUpRight className="size-4 shrink-0 text-sage-deep" aria-hidden />
                    )}
                    <span className="font-medium text-ink">
                      {movement.kind === 'dispense'
                        ? (movement.treatment_item_name ?? t('pharmacy.stock'))
                        : (movement.reason ?? t('pharmacy.correction'))}
                    </span>
                  </span>
                  <span className="numeric shrink-0 text-sm font-semibold">
                    {formatQuantity(movement.quantity)} {record.unit ?? ''}
                  </span>
                </div>

                <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-faint">
                  <span className="numeric">{dateTime(movement.moved_at)}</span>

                  {/* A dispense links to everyone it touched. */}
                  {movement.patient_id ? (
                    <Link
                      to="/patients/$id"
                      params={{ id: String(movement.patient_id) }}
                      className="text-rust hover:underline"
                    >
                      {movement.patient_name ?? t('field.patient')}
                    </Link>
                  ) : null}
                  {movement.customer_id ? (
                    <Link
                      to="/customers/$id"
                      params={{ id: String(movement.customer_id) }}
                      className="text-rust hover:underline"
                    >
                      {movement.customer_name ?? t('customers.title')}
                    </Link>
                  ) : null}
                  {movement.treatment_id ? (
                    <Link
                      to="/treatments/$id"
                      params={{ id: String(movement.treatment_id) }}
                      className="text-rust hover:underline"
                    >
                      {t('treatments.title')}
                    </Link>
                  ) : null}
                  {movement.invoice_number ? (
                    <span className="numeric">
                      {movement.invoice_number}
                      {movement.invoice_status
                        ? ` · ${t(`invoices.status_${movement.invoice_status}`)}`
                        : ''}
                    </span>
                  ) : null}
                  {movement.reverses_movement_id ? (
                    <span className="rounded-full bg-sunken px-2 py-0.5">
                      {t('pharmacy.reversal')}
                    </span>
                  ) : null}
                </div>
              </li>
            )
          })}
          {record.movements.length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>

      <Dialog
        open={correctionOpen}
        onOpenChange={setCorrectionOpen}
        title={t('pharmacy.correction')}
        description={t('pharmacy.correctionHint')}
        footer={
          <>
            <Button onClick={() => setCorrectionOpen(false)}>{t('action.cancel')}</Button>
            <Button
              variant="primary"
              disabled={newRemaining === null || correct.isPending}
              onClick={() =>
                newRemaining !== null &&
                correct.mutate({
                  id: lotId,
                  data: { new_remaining: newRemaining, reason: reason || null },
                })
              }
            >
              {t('action.confirm')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <NumberInput
            label={t('pharmacy.remaining')}
            value={newRemaining}
            unit={record.unit ?? undefined}
            error={correct.isError ? t('value.mustNotBeZero') : undefined}
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
        </div>
      </Dialog>
    </div>
  )
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-0.5 text-sm text-ink">{children}</dd>
    </div>
  )
}
