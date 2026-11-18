import { Link } from '@tanstack/react-router'
import { CalendarDays } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useListAppointments } from '@/api/generated/endpoints'
import { MapsLink } from '@/components/MapsLink'
import {
  IncompleteBadge,
  NotBilledBadge,
  PdfStaleBadge,
  RemindersBadge,
} from '@/components/RecordBadges'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'
import { addressLine, visitDestination } from '@/lib/maps'

/**
 * Today's appointments, in time order, the ones already over included: each opens its
 * appointment, and carries the address with the drive there in Google Maps — the Maps app on
 * the phone. The chips are the ones Termine shows, so today's work not yet billed says so here.
 */
export function TodayCard() {
  const { t } = useTranslation()
  const { time } = useLocaleFormat()
  const { default_country: defaultCountry } = useOperatorConfig()
  // Fifty is far more than a practice day holds — the list's own default would be a hundred.
  const appointments = useListAppointments({ when: 'today', limit: 50 })
  const rows = appointments.data ?? []

  return (
    <section className="rounded-card border border-line bg-surface p-4">
      <h2 className="eyebrow flex items-center gap-1.5">
        <CalendarDays className="size-3.5" />
        {t('dashboard.today')}
        {rows.length > 0 ? (
          <span className="numeric ml-auto rounded-full bg-cream-soft px-2 py-0.5 text-xs font-bold text-ink-soft">
            {rows.length}
          </span>
        ) : null}
      </h2>
      {appointments.isSuccess && rows.length === 0 ? (
        <p className="mt-2 text-sm text-ink-faint">{t('dashboard.noAppointmentsToday')}</p>
      ) : null}
      <ul className="mt-1 flex flex-col divide-y divide-line">
        {rows.map((row) => {
          const destination = visitDestination(row.address, defaultCountry)
          return (
            <li key={row.id} className="py-2">
              {/* The row opens the appointment; the route is a link of its own beside it, never
                  inside it. */}
              <Link
                to="/appointments/$id"
                params={{ id: String(row.id) }}
                className="flex items-baseline gap-3 hover:text-rust"
              >
                <span className="numeric w-12 shrink-0 text-sm font-semibold text-ink">
                  {time(row.starts_at) || '—'}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm text-ink">
                    {row.customer_name || row.calendar_title || '—'}
                  </span>
                  {row.patient_names.length > 0 ? (
                    <span className="block truncate text-xs text-ink-faint">
                      {row.patient_names.join(', ')}
                    </span>
                  ) : null}
                </span>
                <span className="inline-flex shrink-0 flex-wrap justify-end gap-1">
                  <IncompleteBadge missing={row.missing_fields} />
                  <NotBilledBadge count={row.unbilled ? 1 : 0} />
                  <PdfStaleBadge stale={row.pdf_stale} />
                  <RemindersBadge count={row.reminder_count} />
                </span>
              </Link>
              {destination ? (
                <span className="mt-0.5 flex flex-wrap items-baseline gap-x-2 pl-15 text-xs text-ink-faint">
                  <span className="min-w-0 break-words">
                    {row.address ? addressLine(row.address) : null}
                  </span>
                  <MapsLink address={destination}>{t('maps.route')}</MapsLink>
                </span>
              ) : null}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
