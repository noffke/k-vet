import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus, Snowflake, Syringe } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListDrugsQueryKey,
  useCreateDrug,
  useListDrugs,
  usePatchDrug,
} from '@/api/generated/endpoints'
import type { Drug } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'

/** The drug cabinet: what the practice stocks, and how much of it is left. */
export function PharmacyPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const { quantity, percent } = useLocaleFormat()
  const [search, setSearch] = useState('')
  const [showArchived, setShowArchived] = useState(false)

  const drugs = useListDrugs({ q: search || undefined, archived: showArchived || undefined })
  const { vat_rates } = useOperatorConfig()
  const patchDrug = usePatchDrug()
  const createDrug = useCreateDrug({
    mutation: {
      onSuccess: async (drug) => {
        // Almost every drug is billed at the standard rate, so the new record starts there
        // instead of on "—" (the operator's first configured rate).
        const standardRate = vat_rates[0]
        if (standardRate) {
          await patchDrug.mutateAsync({ id: drug.id, data: { vat_percent: standardRate } })
        }
        await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
        await navigate({ to: '/pharmacy/$id', params: { id: String(drug.id) } })
      },
    },
  })

  const columns: DataListColumn<Drug>[] = [
    {
      id: 'name',
      header: t('field.name'),
      primary: true,
      cell: (row) => (
        <span className="inline-flex items-center gap-1.5">
          {row.name ?? '—'}
          {row.vaccine ? (
            <Syringe className="size-3.5 text-ink-faint" aria-label={t('pharmacy.flags.vaccine')} />
          ) : null}
          {row.refrigerate ? (
            <Snowflake
              className="size-3.5 text-ink-faint"
              aria-label={t('pharmacy.flags.refrigerate')}
            />
          ) : null}
          {row.narcotic ? (
            <span className="rounded bg-rust-soft px-1 text-[0.625rem] font-bold text-rust">
              {t('pharmacy.flags.narcotic')}
            </span>
          ) : null}
        </span>
      ),
    },
    {
      id: 'manufacturer',
      header: t('field.manufacturer'),
      cell: (row) => row.manufacturer_name ?? '',
    },
    { id: 'vat', header: t('field.vat'), numeric: true, cell: (row) => percent(row.vat_percent) },
    {
      id: 'stock',
      header: t('pharmacy.stock'),
      numeric: true,
      cell: (row) => quantity(row.in_stock),
    },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('pharmacy.title')}
        actions={
          <Button variant="accent" onClick={() => createDrug.mutate()}>
            <Plus className="size-4" />
            {t('pharmacy.newDrug')}
          </Button>
        }
      />

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
        data={drugs.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={drugs.isPending}
        error={drugs.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) => void navigate({ to: '/pharmacy/$id', params: { id: String(row.id) } })}
      />
    </div>
  )
}
