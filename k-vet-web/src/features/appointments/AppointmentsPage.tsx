import * as Tabs from '@radix-ui/react-tabs'
import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListAppointmentsQueryKey,
  listAppointments,
  useCreateAppointment,
} from '@/api/generated/endpoints'
import type { Appointment, ListAppointmentsWhen } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { LoadMore } from '@/components/LoadMore'
import { PageHeader } from '@/components/PageHeader'
import { IncompleteBadge, NotBilledBadge, PdfStaleBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { formatDayHeading, todayIso } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { usePagedList } from '@/lib/paging'
import { scheduleOf } from '@/lib/schedule'
import { cn } from '@/lib/utils'

/** The day after `iso` (`yyyy-MM-dd`), as an ISO date. */
function nextDay(iso: string): string {
  const [year = 0, month = 1, day = 1] = iso.split('-').map(Number)
  return new Date(Date.UTC(year, month - 1, day + 1)).toISOString().slice(0, 10)
}

/**
 * The practice's appointments, in two tabs: what is coming — from today on, earliest first,
 * under a heading per day, the undated drafts on top — and what was, newest first. One long
 * list of both had the next visit buried among the past ones.
 *
 * "New appointment" creates the record immediately and opens it — there is no create form to
 * abandon. It arrives empty; the detail page offers today's date and waits for a time, which is
 * what makes the appointment complete.
 */
export function AppointmentsPage() {
  const { t } = useTranslation()
  const { dateTime, time, locale } = useLocaleFormat()
  const navigate = useNavigate()
  const client = useQueryClient()
  const tab: ListAppointmentsWhen = useSearch({ from: '/app/appointments' }).when ?? 'upcoming'
  const upcoming = tab === 'upcoming'
  const [search, setSearch] = useState('')

  const params = { q: search || undefined, when: tab }
  const appointments = usePagedList(getListAppointmentsQueryKey(params), listAppointments, params)
  const createAppointment = useCreateAppointment({
    mutation: {
      onSuccess: async (appointment) => {
        await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
        await navigate({ to: '/appointments/$id', params: { id: String(appointment.id) } })
      },
    },
  })

  const today = todayIso()
  const tomorrow = nextDay(today)
  /** "Heute · Di., 27.10.", "Morgen · …", "Fr., 30.10." — or the drafts' "Ohne Datum". */
  const dayOf = (row: Appointment): { key: string; label: ReactNode } => {
    if (!row.starts_at) return { key: 'undated', label: t('appointments.undated') }
    const day = scheduleOf(row.starts_at).date
    const heading = formatDayHeading(day, locale, today)
    const label =
      day === today
        ? `${t('appointments.today')} · ${heading}`
        : day === tomorrow
          ? `${t('appointments.tomorrow')} · ${heading}`
          : heading
    return { key: day, label }
  }

  const columns: DataListColumn<Appointment>[] = [
    {
      id: 'starts_at',
      header: t('field.startsAt'),
      primary: true,
      // Under its day's heading the time is enough; the past list has no headings.
      cell: (row) => (upcoming ? time(row.starts_at) : dateTime(row.starts_at)) || '—',
    },
    // Whose visit and which animals: what the vet looks for in the list. The note is on the
    // appointment itself (and still found by the search).
    {
      id: 'customer',
      header: t('appointments.customer'),
      cell: (row) => row.customer_name || '—',
    },
    {
      id: 'patients',
      header: t('field.patients'),
      cell: (row) => row.patient_names.join(', ') || '—',
    },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('appointments.title')}
        actions={
          <Button
            variant="accent"
            onClick={() =>
              createAppointment.mutate({
                data: { starts_at: null, note: null },
              })
            }
            disabled={createAppointment.isPending}
          >
            <Plus className="size-4" />
            {t('appointments.new')}
          </Button>
        }
      />

      <Tabs.Root
        value={tab}
        onValueChange={(value) =>
          void navigate({
            to: '/appointments',
            search: value === 'past' ? { when: 'past' } : {},
            replace: true,
          })
        }
      >
        <div className="mt-4 flex flex-wrap items-end gap-3 border-b border-line">
          <Tabs.List aria-label={t('appointments.title')} className="flex gap-1">
            {(['upcoming', 'past'] as const).map((value) => (
              <Tabs.Trigger
                key={value}
                value={value}
                className={cn(
                  'flex min-h-11 items-center border-b-2 border-transparent px-3 text-sm font-medium text-ink-soft sm:min-h-9',
                  'hover:text-ink data-[state=active]:border-rust data-[state=active]:text-rust',
                )}
              >
                {t(value === 'upcoming' ? 'appointments.upcoming' : 'appointments.past')}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
        </div>
        <Tabs.Content value={tab}>
          <input
            type="search"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('list.searchPlaceholder')}
            className="mt-4 w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9 sm:max-w-sm"
          />

          <DataList
            className="mt-4"
            data={appointments.rows}
            columns={columns}
            getRowId={(row) => String(row.id)}
            groupBy={upcoming ? dayOf : undefined}
            isLoading={appointments.isPending}
            error={appointments.isError ? t('list.error') : null}
            emptyMessage={t(upcoming ? 'appointments.noUpcoming' : 'appointments.noPast')}
            rowBadge={(row) => (
              <span className="inline-flex flex-wrap items-center gap-1">
                <IncompleteBadge missing={row.missing_fields} />
                <NotBilledBadge count={row.unbilled ? 1 : 0} />
                <PdfStaleBadge stale={row.pdf_stale} />
              </span>
            )}
            onRowClick={(row) =>
              void navigate({ to: '/appointments/$id', params: { id: String(row.id) } })
            }
          />
          <LoadMore list={appointments} />
        </Tabs.Content>
      </Tabs.Root>
    </div>
  )
}
