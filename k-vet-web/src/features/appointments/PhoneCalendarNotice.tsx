import { CalendarClock, EyeOff, UserRound } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useSuggestAppointmentCustomers } from '@/api/generated/endpoints'
import type { Appointment } from '@/api/generated/model'
import { Button } from '@/components/ui/button'

interface PhoneCalendarNoticeProps {
  appointment: Appointment
  onPickCustomer: (customerId: number) => void
  onShowAgain: () => void
}

/**
 * What the phone's calendar did to this appointment (migration 0036). Made there, it has no
 * customer yet: its title is shown with the customers it may mean — named there, or one of
 * their animals — one click each, never assigned silently. Deleted there although something
 * was recorded on it, it stays here and can go back into the calendar.
 */
export function PhoneCalendarNotice({
  appointment,
  onPickCustomer,
  onShowAgain,
}: PhoneCalendarNoticeProps) {
  const { t } = useTranslation()
  const needsCustomer = appointment.from_phone && appointment.customer_id == null
  const suggestions = useSuggestAppointmentCustomers(appointment.id, {
    query: { enabled: needsCustomer },
  })

  return (
    <>
      {needsCustomer ? (
        <aside className="mt-4 rounded-card border border-line-strong bg-cream-soft px-4 py-3">
          <p className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wider text-ink-soft">
            <CalendarClock className="size-4" aria-hidden />
            {t('calendar.fromPhone')}
          </p>
          <p className="mt-1 text-sm font-medium text-ink">
            {appointment.calendar_title
              ? t('calendar.title', { title: appointment.calendar_title })
              : t('calendar.noTitle')}
          </p>
          {(suggestions.data ?? []).length > 0 ? (
            <>
              <p className="mt-2 text-xs text-ink-soft">{t('calendar.suggestions')}</p>
              <ul className="mt-1 flex flex-wrap gap-2">
                {(suggestions.data ?? []).map((customer) => (
                  <li key={customer.id}>
                    <Button onClick={() => onPickCustomer(customer.id)}>
                      <UserRound className="size-4" />
                      {customer.name}
                      {customer.patients.length > 0 ? (
                        <span className="text-ink-soft"> · {customer.patients.join(', ')}</span>
                      ) : null}
                    </Button>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="mt-1 text-xs text-ink-soft">{t('calendar.pickBelow')}</p>
          )}
        </aside>
      ) : null}
      {appointment.calendar_hidden ? (
        <aside
          role="alert"
          className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-card border border-rust/40 bg-rust-soft/40 px-4 py-3"
        >
          <p className="flex items-center gap-1.5 text-sm text-ink">
            <EyeOff className="size-4 text-rust" aria-hidden />
            {t('calendar.hidden')}
          </p>
          <Button onClick={onShowAgain}>{t('calendar.showAgain')}</Button>
        </aside>
      ) : null}
    </>
  )
}
