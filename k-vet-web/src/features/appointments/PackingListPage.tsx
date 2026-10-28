import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useSearch } from '@tanstack/react-router'
import { PackageX, Printer } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  getGetPackingListQueryKey,
  getListRemindersQueryKey,
  useGetPackingList,
  usePatchReminder,
} from '@/api/generated/endpoints'
import type { PackingTotal, Reminder } from '@/api/generated/model'
import { DateInput } from '@/components/DateInput'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { todayIso } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { refusalKey } from '@/lib/refusal'
import { cn } from '@/lib/utils'

/** The day after `iso` (`yyyy-MM-dd`), as an ISO date. */
function nextDay(iso: string): string {
  const [year = 0, month = 1, day = 1] = iso.split('-').map(Number)
  return new Date(Date.UTC(year, month - 1, day + 1)).toISOString().slice(0, 10)
}

/**
 * What to take along for a day: every drug the day's appointments are to get ("Mitbringen"),
 * added up per packaging and checked against the shelf, and the same items visit by visit,
 * ticked off while packing — saved at once, so what was packed on the laptop in the evening
 * shows on the phone in the car.
 */
export function PackingListPage() {
  const { t } = useTranslation()
  const { date: formatDate, time, quantity } = useLocaleFormat()
  const navigate = useNavigate()
  const client = useQueryClient()
  const today = todayIso()
  const date = useSearch({ from: '/app/packing-list' }).date ?? today
  const list = useGetPackingList({ date })
  const pack = usePatchReminder({
    mutation: {
      onSettled: async (reminder) => {
        await client.invalidateQueries({ queryKey: getGetPackingListQueryKey() })
        if (reminder) {
          await client.invalidateQueries({
            queryKey: getListRemindersQueryKey(reminder.appointment_id),
          })
        }
      },
      onError: (error) => toast.error(t(refusalKey(error))),
    },
  })
  const show = (day: string | null) =>
    void navigate({ to: '/packing-list', search: day && day !== today ? { date: day } : {} })

  const totals = list.data?.totals ?? []
  const visits = list.data?.appointments ?? []

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('packing.title')}
        actions={
          <Button className="print:hidden" onClick={() => window.print()}>
            <Printer className="size-4" />
            {t('packing.print')}
          </Button>
        }
      >
        <p className="text-sm text-ink-soft">{t('packing.forDay', { date: formatDate(date) })}</p>
      </PageHeader>

      <div className="mt-4 flex flex-wrap items-end gap-2 print:hidden">
        <Button variant={date === today ? 'primary' : undefined} onClick={() => show(today)}>
          {t('appointments.today')}
        </Button>
        <Button
          variant={date === nextDay(today) ? 'primary' : undefined}
          onClick={() => show(nextDay(today))}
        >
          {t('appointments.tomorrow')}
        </Button>
        <DateInput
          label={t('packing.day')}
          value={date}
          wrapperClassName="w-44"
          onChange={(value) => {
            if (value) show(value)
          }}
        />
      </div>

      {list.isPending ? (
        <p className="mt-6 text-sm text-ink-faint">{t('list.loading')}</p>
      ) : list.isError ? (
        <p className="mt-6 text-sm text-danger" role="alert">
          {t('list.error')}
        </p>
      ) : visits.length === 0 ? (
        <p className="mt-6 rounded-card border border-dashed border-line-strong px-4 py-8 text-center text-sm text-ink-faint">
          {t('packing.nothing')}
        </p>
      ) : (
        <>
          <section className="mt-6">
            <h2 className="eyebrow">{t('packing.together')}</h2>
            <ul className="mt-2 flex flex-col divide-y divide-line rounded-card border border-line bg-surface">
              {totals.map((total) => (
                <TotalRow key={total.drug_packaging_id} total={total} />
              ))}
            </ul>
          </section>

          <section className="mt-6">
            <h2 className="eyebrow">{t('packing.byVisit')}</h2>
            <ul className="mt-2 flex flex-col gap-3">
              {visits.map((visit) => (
                <li
                  key={visit.appointment_id}
                  className="rounded-card border border-line bg-surface p-3 break-inside-avoid"
                >
                  <Link
                    to="/appointments/$id"
                    params={{ id: String(visit.appointment_id) }}
                    className="flex flex-wrap items-baseline gap-x-2 text-ink hover:text-rust"
                  >
                    <span className="numeric font-semibold">{time(visit.starts_at)}</span>
                    <span className="font-medium">{visit.customer_name ?? '—'}</span>
                    <span className="text-sm text-ink-soft">{visit.patients.join(', ')}</span>
                  </Link>
                  <ul className="mt-2 flex flex-col gap-1">
                    {visit.reminders.map((reminder) => (
                      <PackRow
                        key={reminder.id}
                        reminder={reminder}
                        onPacked={(packed) => pack.mutate({ id: reminder.id, data: { packed } })}
                        format={quantity}
                      />
                    ))}
                  </ul>
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </div>
  )
}

function TotalRow({ total }: { total: PackingTotal }) {
  const { t } = useTranslation()
  const { quantity } = useLocaleFormat()
  const size = total.size ? `${quantity(total.size)} ${total.unit ?? ''}`.trim() : ''
  return (
    <li className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5 px-3 py-2">
      <span className="text-ink">
        <span className="numeric font-semibold">{quantity(total.quantity)} ×</span> {total.name}
        {size ? <span className="text-ink-soft"> · {size}</span> : null}
      </span>
      {total.short ? (
        <span className="flex items-center gap-1 text-xs font-semibold text-danger">
          <PackageX className="size-3.5" aria-hidden />
          {t('packing.short', {
            stock: `${quantity(total.in_stock)} ${total.unit ?? ''}`.trim(),
          })}
        </span>
      ) : null}
    </li>
  )
}

function PackRow({
  reminder,
  onPacked,
  format,
}: {
  reminder: Reminder
  onPacked: (packed: boolean) => void
  format: (value: string | null | undefined) => string
}) {
  // Ticked at once, saved behind it; the server's answer (or a refusal) puts it right.
  const [packed, setPacked] = useState(reminder.packed)
  useEffect(() => setPacked(reminder.packed), [reminder.packed])
  const size = reminder.size ? `${format(reminder.size)} ${reminder.unit ?? ''}`.trim() : ''
  return (
    <li>
      <label
        className={cn(
          'flex min-h-11 items-center gap-2 text-sm sm:min-h-9',
          packed ? 'text-ink-faint line-through' : 'text-ink',
        )}
      >
        <input
          type="checkbox"
          className="size-4 accent-ink"
          checked={packed}
          onChange={(event) => {
            setPacked(event.target.checked)
            onPacked(event.target.checked)
          }}
        />
        <span>
          <span className="numeric font-semibold">{format(reminder.quantity)} ×</span>{' '}
          {reminder.name}
          {size ? ` · ${size}` : ''}
          {reminder.note ? <span className="text-ink-soft"> — {reminder.note}</span> : null}
        </span>
      </label>
    </li>
  )
}
