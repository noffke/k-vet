import { Link } from '@tanstack/react-router'
import {
  CalendarClock,
  CircleCheck,
  FileWarning,
  MailWarning,
  ReceiptText,
  Stamp,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useGetDashboard } from '@/api/generated/endpoints'
import type { AtRisk } from '@/api/generated/model'
import { PageHeader } from '@/components/PageHeader'
import { FailedMailsCard } from '@/features/dashboard/FailedMailsCard'
import { useLocaleFormat } from '@/lib/locale'

/**
 * Landing page: what quietly piles up (FR-036). Three of the widgets track money that has not
 * arrived — work nobody billed, invoices nobody released, invoices that never reached the
 * customer — and every row is a link to the page where that case is settled.
 *
 * Only what has something in it is shown (issues.md): an empty section is no news, and a page
 * of "0"s hides the one that is not. With nothing at all, it says so once.
 */
export function DashboardPage() {
  const { t } = useTranslation()
  const { date, quantity } = useLocaleFormat()
  const dashboard = useGetDashboard()

  const data = dashboard.data
  const pendingCount = data?.pending_invoice_count ?? 0
  const lots = data?.expiring_lots ?? []
  const nothing =
    data !== undefined &&
    data.failed_mails.length === 0 &&
    data.stale_pdfs.count === 0 &&
    data.unbilled.count === 0 &&
    data.unreleased.count === 0 &&
    data.unsent.count === 0 &&
    pendingCount === 0 &&
    lots.length === 0

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader eyebrow={t('app.practice')} title={t('dashboard.title')} />

      {dashboard.isError ? (
        <p className="mt-5 text-sm text-danger" role="alert">
          {t('list.error')}
        </p>
      ) : null}
      {dashboard.isPending ? (
        <p className="mt-5 text-sm text-ink-faint">{t('list.loading')}</p>
      ) : null}

      {nothing ? (
        <p className="mt-5 flex items-center gap-2 rounded-card border border-line bg-surface p-4 text-sm text-ink-soft">
          <CircleCheck className="size-4 shrink-0 text-sage-deep" aria-hidden />
          {t('dashboard.allDone')}
        </p>
      ) : null}

      {data && !nothing ? (
        <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {/* First: an invoice mail that failed needs a decision, and nothing else would say so. */}
          <FailedMailsCard mails={data.failed_mails} />
          {/* Then an invoice whose PDF no longer shows what it bills: it must be updated before
              it goes out. */}
          <AtRiskCard
            icon={<FileWarning className="size-3.5" />}
            title={t('dashboard.stalePdfs')}
            hint={t('dashboard.stalePdfsHint')}
            data={data.stale_pdfs}
          />
          {/* In the order the money moves: worked, billed, sent. */}
          <AtRiskCard
            icon={<ReceiptText className="size-3.5" />}
            title={t('dashboard.unbilled')}
            hint={t('dashboard.unbilledHint')}
            data={data.unbilled}
          />
          <AtRiskCard
            icon={<Stamp className="size-3.5" />}
            title={t('dashboard.unreleased')}
            hint={t('dashboard.unreleasedHint')}
            data={data.unreleased}
          />
          <AtRiskCard
            icon={<MailWarning className="size-3.5" />}
            title={t('dashboard.unsent')}
            hint={t('dashboard.unsentHint')}
            data={data.unsent}
          />

          {pendingCount > 0 ? (
            <Link
              to="/invoices"
              className="rounded-card border border-line bg-cream-soft p-4 hover:border-rust/40"
            >
              <span className="eyebrow flex items-center gap-1.5">
                <ReceiptText className="size-3.5" />
                {t('dashboard.pendingInvoices')}
              </span>
              <span className="numeric mt-1 block text-4xl font-bold text-rust">
                {pendingCount}
              </span>
              <span className="mt-1 block text-sm text-ink-soft">
                {t('dashboard.pendingInvoicesHint')}
              </span>
            </Link>
          ) : null}

          {lots.length > 0 ? (
            <section className="rounded-card border border-line bg-surface p-4 sm:col-span-2">
              <h2 className="eyebrow flex items-center gap-1.5">
                <CalendarClock className="size-3.5" />
                {t('dashboard.expiringLots')}
              </h2>
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
            </section>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

/**
 * One case of money that has not arrived: the oldest few, and how many there are in all.
 * Nothing when there is no case — the page leaves out what has nothing in it.
 */
function AtRiskCard({
  icon,
  title,
  hint,
  data,
}: {
  icon: React.ReactNode
  title: string
  hint: string
  data: AtRisk
}) {
  const { t } = useTranslation()
  const { date, dateTime, money } = useLocaleFormat()
  const entries = data.entries
  const count = data.count
  if (count === 0) return null

  return (
    <section className="rounded-card border border-line bg-surface p-4">
      <h2 className="eyebrow flex items-center gap-1.5">
        {icon}
        {title}
        <span className="numeric ml-auto rounded-full bg-rust-soft px-2 py-0.5 text-xs font-bold text-rust">
          {count}
        </span>
      </h2>

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
                {/* Waiting on purpose — "Später versenden" — rather than forgotten. */}
                {entry.mail_scheduled_for ? (
                  <span className="block truncate text-xs text-ink-soft">
                    {t('invoices.mailScheduled', { time: dateTime(entry.mail_scheduled_for) })}
                  </span>
                ) : null}
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
    </section>
  )
}
