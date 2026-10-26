import { Calculator } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { applyQuote, useGetQuote, useListCustomerQuotes } from '@/api/generated/endpoints'
import type { QuotePrices, TreatmentItem } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useLocaleFormat } from '@/lib/locale'
import { refusalKey } from '@/lib/refusal'
import { scheduleOf } from '@/lib/schedule'
import { cn } from '@/lib/utils'

interface ApplyQuoteProps {
  treatmentId: number
  customerId: number | null | undefined
  /** The animal whose tab it is; a per-visit position goes to the visit by itself. */
  patientTreatmentId: number
  disabled?: boolean
  /** The treatment's lines afterwards, and the quote's name for the undo. */
  onApplied: (applied: TreatmentItem[], name: string) => void
}

/**
 * Takes a customer's Preisauskunft into the visit: its positions, quantities, factors and
 * kilometres, at the prices quoted. Where a price has moved since, it is shown before anything
 * is added, and the vet may take today's instead — for a GOT position the schedule's current
 * fee is the one that binds.
 */
export function ApplyQuote({
  treatmentId,
  customerId,
  patientTreatmentId,
  disabled,
  onApplied,
}: ApplyQuoteProps) {
  const { t } = useTranslation()
  const { date, money } = useLocaleFormat()
  const quotes = useListCustomerQuotes(customerId ?? 0, { query: { enabled: Boolean(customerId) } })
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<number | null>(null)
  const [prices, setPrices] = useState<QuotePrices>('quoted')
  const [busy, setBusy] = useState(false)
  const quote = useGetQuote(selected ?? 0, { query: { enabled: selected !== null } })
  const rows = quotes.data ?? []
  if (rows.length === 0) return null

  const close = () => {
    setOpen(false)
    setSelected(null)
    setPrices('quoted')
  }

  const take = async () => {
    if (!quote.data) return
    setBusy(true)
    try {
      const applied = await applyQuote(treatmentId, {
        quote_id: quote.data.id,
        patient_treatment_id: patientTreatmentId,
        prices,
      })
      onApplied(applied, quote.data.name ?? t('quote.untitled'))
      close()
    } catch (error) {
      // A drug the lots no longer cover, say: the quote goes in whole or not at all.
      toast.error(t(refusalKey(error)))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Button className="mt-2" disabled={disabled} onClick={() => setOpen(true)}>
        <Calculator className="size-4" />
        {t('quote.applyButton')}
      </Button>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (!next) close()
        }}
        title={t('quote.applyTitle')}
        footer={
          <>
            {selected !== null ? (
              <Button disabled={busy} onClick={() => setSelected(null)}>
                {t('quote.applyBack')}
              </Button>
            ) : null}
            <Button disabled={busy} onClick={close}>
              {t('action.cancel')}
            </Button>
            {selected !== null ? (
              <Button variant="primary" disabled={busy || !quote.data} onClick={() => void take()}>
                {t('quote.applyConfirm')}
              </Button>
            ) : null}
          </>
        }
      >
        {selected === null ? (
          <ul className="flex flex-col gap-2">
            {rows.map((row) => (
              <li key={row.id}>
                <button
                  type="button"
                  onClick={() => setSelected(row.id)}
                  className="flex min-h-11 w-full flex-wrap items-baseline justify-between gap-x-3 gap-y-1 rounded-card border border-line bg-surface px-3 py-2 text-left hover:bg-cream-soft"
                >
                  <span className="numeric text-sm font-medium text-ink">
                    {date(scheduleOf(row.created_at).date)}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">
                    {row.name ?? (row.positions.join(', ') || t('quote.untitled'))}
                  </span>
                  <span className="numeric text-sm text-ink">{money(row.total_gross)}</span>
                  {row.prices_changed ? (
                    <span className="text-xs font-semibold text-rust">
                      {t('quote.pricesChanged')}
                    </span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        ) : quote.data ? (
          <div className="flex flex-col gap-3">
            <ul className="flex flex-col gap-1.5">
              {quote.data.lines.map((line, index) => (
                <li
                  // biome-ignore lint/suspicious/noArrayIndexKey: a quote may list a position twice
                  key={index}
                  className={cn(
                    'rounded-control px-2 py-1 text-sm',
                    line.price_changed && 'bg-blush/40',
                  )}
                >
                  <div className="flex items-baseline justify-between gap-3">
                    <span className="min-w-0 flex-1 text-ink">{line.name}</span>
                    <span className="numeric text-ink">{money(line.line_gross)}</span>
                  </div>
                  {line.price_changed ? (
                    <p className="numeric text-xs font-semibold text-rust">
                      {line.today_line_gross
                        ? t('quote.priceChanged', {
                            today: money(line.today_line_gross),
                            quoted: money(line.line_gross),
                          })
                        : t('quote.priceGone')}
                      {line.got_number ? ` ${t('quote.gotNote')}` : ''}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
            <p className="numeric text-right text-sm font-semibold text-ink">
              {t('quote.total')}: {money(quote.data.total_gross)}
            </p>
            {quote.data.prices_changed ? (
              <fieldset className="border-0 p-0">
                <legend className="text-xs font-semibold text-ink-soft">
                  {t('quote.applyPrices')}
                </legend>
                {(['quoted', 'today'] as const).map((option) => (
                  <label
                    key={option}
                    className="flex min-h-11 items-center gap-2 text-sm text-ink sm:min-h-9"
                  >
                    <input
                      type="radio"
                      name="quote-prices"
                      className="size-4 accent-ink"
                      checked={prices === option}
                      onChange={() => setPrices(option)}
                    />
                    {option === 'quoted'
                      ? t('quote.pricesQuoted', { amount: money(quote.data.total_gross) })
                      : t('quote.pricesToday', { amount: money(quote.data.today_total_gross) })}
                  </label>
                ))}
              </fieldset>
            ) : null}
          </div>
        ) : (
          <p className="text-sm text-ink-faint">{t('list.loading')}</p>
        )}
      </Dialog>
    </>
  )
}
