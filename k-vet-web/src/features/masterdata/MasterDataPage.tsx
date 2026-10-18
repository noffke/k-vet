import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListManufacturersQueryKey,
  getListSuppliersQueryKey,
  listManufacturers,
  listSuppliers,
  useCreateManufacturer,
  useCreateSupplier,
} from '@/api/generated/endpoints'
import type { AddressBookEntry } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { LoadMore } from '@/components/LoadMore'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { type PagedList, usePagedList } from '@/lib/paging'

/**
 * Hersteller and Lieferanten: the two address books the pharmacy picks from.
 *
 * They share one screen because they are only ever met together — while entering a drug,
 * where one names who makes it and the other who sold it.
 */
export function MasterDataPage() {
  const { t } = useTranslation()
  const [showArchived, setShowArchived] = useState(false)

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader eyebrow={t('app.practice')} title={t('masterData.title')} />

      <div className="mt-4">
        <CheckboxField
          label={t('record.showArchived')}
          checked={showArchived}
          onChange={(event) => setShowArchived(event.target.checked)}
        />
      </div>

      <ManufacturerSection showArchived={showArchived} />
      <SupplierSection showArchived={showArchived} />
    </div>
  )
}

function ManufacturerSection({ showArchived }: { showArchived: boolean }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()

  const params = { archived: showArchived || undefined }
  const manufacturers = usePagedList(
    getListManufacturersQueryKey(params),
    listManufacturers,
    params,
  )
  const create = useCreateManufacturer({
    mutation: {
      onSuccess: async (entry) => {
        await client.invalidateQueries({ queryKey: getListManufacturersQueryKey() })
        await navigate({
          to: '/masterdata/manufacturers/$id',
          params: { id: String(entry.id) },
        })
      },
    },
  })

  return (
    <AddressBookSection
      title={t('masterData.manufacturers')}
      newLabel={t('masterData.newManufacturer')}
      list={manufacturers}
      onCreate={() => create.mutate()}
      onOpen={(id) =>
        void navigate({ to: '/masterdata/manufacturers/$id', params: { id: String(id) } })
      }
    />
  )
}

function SupplierSection({ showArchived }: { showArchived: boolean }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()

  const params = { archived: showArchived || undefined }
  const suppliers = usePagedList(getListSuppliersQueryKey(params), listSuppliers, params)
  const create = useCreateSupplier({
    mutation: {
      onSuccess: async (entry) => {
        await client.invalidateQueries({ queryKey: getListSuppliersQueryKey() })
        await navigate({ to: '/masterdata/suppliers/$id', params: { id: String(entry.id) } })
      },
    },
  })

  return (
    <AddressBookSection
      title={t('masterData.suppliers')}
      newLabel={t('masterData.newSupplier')}
      list={suppliers}
      onCreate={() => create.mutate()}
      onOpen={(id) =>
        void navigate({ to: '/masterdata/suppliers/$id', params: { id: String(id) } })
      }
    />
  )
}

/** One address book: a heading, its own "new" button and the list itself. */
function AddressBookSection({
  title,
  newLabel,
  list,
  onCreate,
  onOpen,
}: {
  title: string
  newLabel: string
  list: PagedList<AddressBookEntry>
  onCreate: () => void
  onOpen: (id: number) => void
}) {
  const { t } = useTranslation()

  // Both address books share these widths so their columns line up down the page.
  const columns: DataListColumn<AddressBookEntry>[] = [
    {
      id: 'name',
      header: t('field.name'),
      primary: true,
      width: 'w-2/3',
      cell: (row) => row.name ?? '—',
    },
    { id: 'city', header: t('field.city'), width: 'w-1/3', cell: (row) => row.addr_city ?? '' },
  ]

  return (
    <section className="mt-6">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-base">{title}</h2>
        <Button variant="accent" size="small" onClick={onCreate}>
          <Plus className="size-4" />
          {newLabel}
        </Button>
      </div>

      <DataList
        className="mt-3"
        data={list.rows}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={list.isPending}
        error={list.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) => onOpen(row.id)}
      />
      <LoadMore list={list} />
    </section>
  )
}
