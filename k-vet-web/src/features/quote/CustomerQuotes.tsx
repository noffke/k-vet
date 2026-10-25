import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useListCustomerQuotes } from '@/api/generated/endpoints'
import { useLocaleFormat } from '@/lib/locale'
import { scheduleOf } from '@/lib/schedule'

/** A customer's kept Preisauskünfte, newest first, each opening the quote. */
export function CustomerQuotes({ customerId }: { customerId: number }) {
  const { t } = useTranslation()
  const { date, money } = useLocaleFormat()
  const quotes = useListCustomerQuotes(customerId)
  const rows = quotes.data ?? []
  if (rows.length === 0) return null

  return (
    <section className="mt-6">
      <h2 className="text-base">{t('quote.customerTitle')}</h2>
      <ul className="mt-2 flex flex-col gap-2">
        {rows.map((quote) => (
          <li key={quote.id}>
            <Link
              to="/quote"
              search={{ id: quote.id }}
              className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 rounded-card border border-line bg-surface px-3 py-2 hover:bg-cream-soft"
            >
              <span className="numeric text-sm font-medium text-ink">
                {/* The day where the practice is, not the UTC one. */}
                {date(scheduleOf(quote.created_at).date)}
              </span>
              <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">
                {quote.name ?? (quote.positions.join(', ') || t('quote.untitled'))}
              </span>
              <span className="numeric text-sm text-ink">{money(quote.total_gross)}</span>
              {quote.prices_changed ? (
                <span className="text-xs font-semibold text-rust">{t('quote.pricesChanged')}</span>
              ) : null}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  )
}
