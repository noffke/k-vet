import { Link } from '@tanstack/react-router'
import { CalendarClock, ReceiptText } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useGetDashboard } from '@/api/generated/endpoints'
import { PageHeader } from '@/components/PageHeader'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Landing page: the two things that quietly pile up — invoices the bookkeeper has not seen
 * and stock about to expire (FR-036). Both widgets are entry points, not just numbers.
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

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
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

        <section className="rounded-card border border-line bg-surface p-4">
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
                      <span className="numeric block text-xs text-ink-faint">
                        {lot.batch_number ?? '—'} · {quantity(lot.remaining)} {lot.unit ?? ''}
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
