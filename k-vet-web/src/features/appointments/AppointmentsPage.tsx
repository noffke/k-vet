import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListAppointmentsQueryKey,
  useCreateAppointment,
  useListAppointments,
} from '@/api/generated/endpoints'
import type { Appointment } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import { IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { useLocaleFormat } from '@/lib/locale'

/**
 * The practice's day list. "New appointment" creates the record immediately (today's
 * date, time still open) and opens it — there is no create form to abandon.
 */
export function AppointmentsPage() {
  const { t } = useTranslation()
  const { dateTime } = useLocaleFormat()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [search, setSearch] = useState('')

  const appointments = useListAppointments({ q: search || undefined })
  const createAppointment = useCreateAppointment({
    mutation: {
      onSuccess: async (appointment) => {
        await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
        await navigate({ to: '/appointments/$id', params: { id: String(appointment.id) } })
      },
    },
  })

  const columns: DataListColumn<Appointment>[] = [
    {
      id: 'starts_at',
      header: t('field.startsAt'),
      primary: true,
      cell: (row) => dateTime(row.starts_at) || '—',
    },
    { id: 'note', header: t('field.note'), cell: (row) => row.note ?? '' },
    {
      id: 'treatments',
      header: t('appointments.treatments'),
      numeric: true,
      cell: (row) => row.treatment_count,
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

      <input
        type="search"
        value={search}
        onChange={(event) => setSearch(event.target.value)}
        placeholder={t('list.searchPlaceholder')}
        className="mt-4 w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9 sm:max-w-sm"
      />

      <DataList
        className="mt-4"
        data={appointments.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={appointments.isPending}
        error={appointments.isError ? t('list.error') : null}
        rowBadge={(row) => <IncompleteBadge missing={row.missing_fields} />}
        onRowClick={(row) =>
          void navigate({ to: '/appointments/$id', params: { id: String(row.id) } })
        }
      />
    </div>
  )
}
