import { Link } from '@tanstack/react-router'
import { CalendarClock, MailWarning, ReceiptText, Stamp } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useGetDashboard } from '@/api/generated/endpoints'
import type { AtRisk } from '@/api/generated/model'
import { PageHeader } from '@/components/PageHeader'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Landing page: what quietly piles up (FR-036). Three of the widgets track money that has not
 * arrived — work nobody billed, invoices nobody released, invoices that never reached the
 * customer — and every row is a link to the page where that case is settled.
 */
export function DashboardPage() {
  const { t } = useTranslation()
  const { date, quantity } = useLocaleFormat()
  const dashboard = useGetDashboard()

  const pendingCount = dashboard.data?.pending_invoice_count ?? 0
  const lots = dashboard.data?.expiring_lots ?? []

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader eyebrow={t('app.practice')} title={t('dashboard.title')} />

      {dashboard.isError ? (
        <p className="mt-5 text-sm text-danger" role="alert">
          {t('list.error')}
        </p>
      ) : null}

      <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {/* In the order the money moves: worked, billed, sent. */}
        <AtRiskCard
          icon={<ReceiptText className="size-3.5" />}
          title={t('dashboard.unbilled')}
          hint={t('dashboard.unbilledHint')}
          empty={t('dashboard.noUnbilled')}
          data={dashboard.data?.unbilled}
        />
        <AtRiskCard
          icon={<Stamp className="size-3.5" />}
          title={t('dashboard.unreleased')}
          hint={t('dashboard.unreleasedHint')}
          empty={t('dashboard.noUnreleased')}
          data={dashboard.data?.unreleased}
        />
        <AtRiskCard
          icon={<MailWarning className="size-3.5" />}
          title={t('dashboard.unsent')}
          hint={t('dashboard.unsentHint')}
          empty={t('dashboard.noUnsent')}
          data={dashboard.data?.unsent}
        />

        <Link
          to="/invoices"
          className="rounded-card border border-line bg-cream-soft p-4 hover:border-rust/40"
        >
          <span className="eyebrow flex items-center gap-1.5">
            <ReceiptText className="size-3.5" />
            {t('dashboard.pendingInvoices')}
          </span>
          <span className="numeric mt-1 block text-4xl font-bold text-rust">{pendingCount}</span>
          <span className="mt-1 block text-sm text-ink-soft">
            {pendingCount === 0 ? t('invoices.nothingPending') : t('dashboard.pendingInvoicesHint')}
          </span>
        </Link>

        <section className="rounded-card border border-line bg-surface p-4 sm:col-span-2">
          <h2 className="eyebrow flex items-center gap-1.5">
            <CalendarClock className="size-3.5" />
            {t('dashboard.expiringLots')}
          </h2>
          {lots.length === 0 ? (
            <p className="mt-2 text-sm text-ink-soft">{t('dashboard.noExpiring')}</p>
          ) : (
            <ul className="mt-2 flex flex-col divide-y divide-line">
              {lots.map((lot) => (
                <li key={lot.lot_id}>
                  <Link
                    to="/lots/$id"
                    params={{ id: String(lot.lot_id) }}
                    className="flex items-baseline justify-between gap-3 py-2 hover:text-rust"
                  >
                    <span className="min-w-0">
                      <span className="block truncate text-sm text-ink">{lot.drug_name}</span>
                      {/* What is left comes first — that is what decides whether the expiry
                          matters — and the batch number says what it is. */}
                      <span className="numeric block text-xs text-ink-faint">
                        {quantity(lot.remaining)} {lot.unit ?? ''}
                        {lot.batch_number
                          ? ` · ${t('pharmacy.batchShort')} ${lot.batch_number}`
                          : ''}
                      </span>
                    </span>
                    <span className="numeric shrink-0 text-sm text-ink-soft">
                      {date(lot.expiration_date)}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  )
}

/**
 * One case of money that has not arrived: the oldest few, and how many there are in all.
 *
 * Empty is the normal state, and it stays quiet — the rust count only appears when there is
 * something to do, so a coloured dashboard means work rather than decoration.
 */
function AtRiskCard({
  icon,
  title,
  hint,
  empty,
  data,
}: {
  icon: React.ReactNode
  title: string
  hint: string
  empty: string
  data: AtRisk | undefined
}) {
  const { t } = useTranslation()
  const { date, money } = useLocaleFormat()
  const entries = data?.entries ?? []
  const count = data?.count ?? 0

  return (
    <section className="rounded-card border border-line bg-surface p-4">
      <h2 className="eyebrow flex items-center gap-1.5">
        {icon}
        {title}
        {count > 0 ? (
          <span className="numeric ml-auto rounded-full bg-rust-soft px-2 py-0.5 text-xs font-bold text-rust">
            {count}
          </span>
        ) : null}
      </h2>

      {count === 0 ? (
        <p className="mt-2 text-sm text-ink-soft">{empty}</p>
      ) : (
        <>
          <p className="mt-1 text-xs text-ink-faint">{hint}</p>
          <ul className="mt-1 flex flex-col divide-y divide-line">
            {entries.map((entry) => (
              <li key={`${entry.treatment_id}-${entry.invoice_number ?? ''}`}>
                <Link
                  to="/treatments/$id"
                  params={{ id: String(entry.treatment_id) }}
                  className="flex items-baseline justify-between gap-3 py-2 hover:text-rust"
                >
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-ink">{entry.customer_name}</span>
                    <span className="block truncate text-xs text-ink-faint">
                      {entry.patients.join(', ')}
                      {entry.date ? ` · ${date(entry.date)}` : ''}
                    </span>
                  </span>
                  <span className="numeric shrink-0 text-sm font-semibold text-ink-soft">
                    {money(entry.total_gross)}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
          {count > entries.length ? (
            <p className="mt-2 text-xs text-ink-faint">
              {t('dashboard.andMore', { count: count - entries.length })}
            </p>
          ) : null}
        </>
      )}
    </section>
  )
}
