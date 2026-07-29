import { useNavigate } from '@tanstack/react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useListPatients } from '@/api/generated/endpoints'
import type { Patient } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge, WarningIcon } from '@/components/RecordBadges'
import { CheckboxField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/** The animal index — the practice's most-used lookup after the day list. */
export function PatientsPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { date } = useLocaleFormat()
  const [search, setSearch] = useState('')
  const [showArchived, setShowArchived] = useState(false)

  const patients = useListPatients({
    q: search || undefined,
    archived: showArchived || undefined,
  })

  const columns: DataListColumn<Patient>[] = [
    { id: 'name', header: t('field.name'), primary: true, cell: (row) => row.name ?? '—' },
    { id: 'species', header: t('field.species'), cell: (row) => row.species ?? '' },
    { id: 'owner', header: t('customers.title'), cell: (row) => row.customer_name ?? '' },
    {
      id: 'birth',
      header: t('field.dateOfBirth'),
      desktopOnly: true,
      cell: (row) => date(row.date_of_birth),
    },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader eyebrow={t('app.practice')} title={t('patients.title')} />

      <div className="mt-4 flex flex-wrap items-center gap-4">
        <input
          type="search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t('list.searchPlaceholder')}
          className="w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9 sm:max-w-sm"
        />
        <CheckboxField
          label={t('record.showArchived')}
          checked={showArchived}
          onChange={(event) => setShowArchived(event.target.checked)}
        />
      </div>

      <DataList
        className="mt-4"
        data={patients.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={patients.isPending}
        error={patients.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <WarningIcon remark={row.warning_remark} />
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) => void navigate({ to: '/patients/$id', params: { id: String(row.id) } })}
      />
    </div>
  )
}
