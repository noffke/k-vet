import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useListOpenQuotes } from '@/api/generated/endpoints'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Quotes no one attached to a customer — a call broken off, a tab closed. They stay reachable
 * here until the cleanup takes them, 28 days after anyone last worked on them.
 */
export function OpenQuotes({ exclude }: { exclude: number | undefined }) {
  const { t } = useTranslation()
  const { date, dateTime, money } = useLocaleFormat()
  const quotes = useListOpenQuotes()
  const rows = (quotes.data ?? []).filter((quote) => quote.id !== exclude)
  if (rows.length === 0) return null

  return (
    <section className="mt-8">
      <h2 className="text-base">{t('quote.openTitle')}</h2>
      <p className="mt-0.5 text-xs text-ink-faint">{t('quote.openHint')}</p>
      <ul className="mt-2 flex flex-col gap-2">
        {rows.map((quote) => (
          <li key={quote.id}>
            <Link
              to="/quote"
              search={{ id: quote.id }}
              className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 rounded-card border border-line bg-surface px-3 py-2 hover:bg-cream-soft"
            >
              <span className="numeric text-sm font-medium text-ink">
                {dateTime(quote.updated_at)}
              </span>
              <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">
                {[quote.name, quote.positions.join(', '), quote.address]
                  .filter(Boolean)
                  .join(' · ') || t('quote.untitled')}
              </span>
              <span className="numeric text-sm text-ink">{money(quote.total_gross)}</span>
              {quote.removed_on ? (
                <span className="w-full text-xs text-ink-faint">
                  {t('quote.removedOn', { date: date(quote.removed_on) })}
                </span>
              ) : null}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  )
}
