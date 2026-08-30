import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListCustomersQueryKey,
  useCreateCustomer,
  useListCustomers,
} from '@/api/generated/endpoints'
import type { Customer } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import {
  ArchivedBadge,
  IncompleteBadge,
  NotInvoiceableBadge,
  WarningIcon,
} from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'

/**
 * The customer index: search, warning indicators, archived on request (FR-009).
 *
 * Search reaches the street, the phone number and the animals' names as well as the household
 * names — the vet looks a customer up by whichever of those they have in front of them
 * (issues.md 3).
 */
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
  // The draft already carries `[invoice] default_country` — the backend prefills it on create,
  // so the field the vet sees and the value stored agree without a second round-trip.
  const createCustomer = useCreateCustomer({
    mutation: {
      onSuccess: async (customer) => {
        await client.invalidateQueries({ queryKey: getListCustomersQueryKey() })
        await navigate({ to: '/customers/$id', params: { id: String(customer.id) } })
      },
    },
  })

  // City, phone and e-mail gave up half the width to data the vet reads on the detail page
  // anyway. The animals are what identifies a household on the phone; the address follows on a
  // sub-line, where it is not squeezed into a column (issues.md 4).
  const columns: DataListColumn<Customer>[] = [
    {
      // The cell has always shown the whole name, not just the surname (issues.md 8).
      id: 'name',
      header: t('field.name'),
      primary: true,
      cell: (row) => customerName(row) || '—',
    },
    {
      id: 'patients',
      header: t('field.patients'),
      cell: (row) => row.patient_names.join(', '),
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
            {/* Only once the record itself is complete, so one gap is not reported twice. */}
            {row.missing_fields.length === 0 ? (
              <NotInvoiceableBadge missing={row.invoice_missing_fields} />
            ) : null}
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        rowSubline={(row) => addressLine(row) || null}
        onRowClick={(row) =>
          void navigate({ to: '/customers/$id', params: { id: String(row.id) } })
        }
      />
    </div>
  )
}

/** The address an invoice would print, on one line — street first, then ZIP and town. */
function addressLine(customer: Customer): string {
  const [street, zip, city] = customer.has_invoice_address
    ? [customer.invoice_street, customer.invoice_zip, customer.invoice_city]
    : [customer.home_street, customer.home_zip, customer.home_city]
  const town = [zip, city].filter(Boolean).join(' ')
  return [street, town].filter(Boolean).join(' · ')
}

/** "Frau Erika Mustermann", plus the second name when the household has one. */
function customerName(customer: Customer): string {
  const first = [customer.first_name, customer.last_name].filter(Boolean).join(' ')
  if (!customer.has_second_name) return first
  const second = [customer.second_first_name, customer.second_last_name].filter(Boolean).join(' ')
  return `${first} & ${second}`
}
