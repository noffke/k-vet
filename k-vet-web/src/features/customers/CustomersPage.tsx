import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListCustomersQueryKey,
  useCreateCustomer,
  useListCustomers,
  usePatchCustomer,
} from '@/api/generated/endpoints'
import type { Customer } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge, WarningIcon } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useOperatorConfig } from '@/lib/config'

/** The customer index: search, warning indicators, archived on request (FR-009). */
export function CustomersPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [search, setSearch] = useState('')
  const [showArchived, setShowArchived] = useState(false)

  const customers = useListCustomers({
    q: search || undefined,
    archived: showArchived || undefined,
  })
  const { default_country } = useOperatorConfig()
  const patchCustomer = usePatchCustomer()
  const createCustomer = useCreateCustomer({
    mutation: {
      onSuccess: async (customer) => {
        // The practice's own country, so what is stored is what an invoice would print.
        await patchCustomer.mutateAsync({
          id: customer.id,
          data: { home_country: default_country },
        })
        await client.invalidateQueries({ queryKey: getListCustomersQueryKey() })
        await navigate({ to: '/customers/$id', params: { id: String(customer.id) } })
      },
    },
  })

  const columns: DataListColumn<Customer>[] = [
    {
      id: 'name',
      header: t('field.lastName'),
      primary: true,
      cell: (row) => customerName(row) || '—',
    },
    {
      id: 'city',
      header: t('field.city'),
      cell: (row) => [row.home_zip, row.home_city].filter(Boolean).join(' '),
    },
    { id: 'phone', header: t('field.phone'), cell: (row) => row.phone_display ?? '' },
    {
      id: 'emails',
      header: t('field.email'),
      desktopOnly: true,
      cell: (row) => row.emails.map((email) => email.email).join(', '),
    },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('customers.title')}
        actions={
          <Button variant="accent" onClick={() => createCustomer.mutate()}>
            <Plus className="size-4" />
            {t('customers.new')}
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
        data={customers.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={customers.isPending}
        error={customers.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <WarningIcon remark={row.warning_remark} />
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) =>
          void navigate({ to: '/customers/$id', params: { id: String(row.id) } })
        }
      />
    </div>
  )
}

/** "Frau Erika Mustermann", plus the second name when the household has one. */
function customerName(customer: Customer): string {
  const first = [customer.first_name, customer.last_name].filter(Boolean).join(' ')
  if (!customer.has_second_name) return first
  const second = [customer.second_first_name, customer.second_last_name].filter(Boolean).join(' ')
  return `${first} & ${second}`
}
