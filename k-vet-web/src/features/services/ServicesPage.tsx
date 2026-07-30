import { useQueryClient } from '@tanstack/react-query'
import { EyeOff, Plus, Route } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListServicesQueryKey,
  useCreateService,
  useListServices,
  usePatchService,
} from '@/api/generated/endpoints'
import type { Service } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { CheckboxField, SelectField, TextField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'

/**
 * The service catalog: the imported GOT 2022 schedule plus the practice's own positions.
 *
 * The catalog has ~900 GOT entries, so this screen is search-first. Surgical positions are
 * hidden until the toggle asks for them (FR-021).
 */
export function ServicesPage() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const { money, percent } = useLocaleFormat()
  const [search, setSearch] = useState('')
  const [showHidden, setShowHidden] = useState(false)
  const [showArchived, setShowArchived] = useState(false)
  const [editing, setEditing] = useState<Service | null>(null)

  const services = useListServices({
    q: search || undefined,
    hidden: showHidden || undefined,
    archived: showArchived || undefined,
  })
  const refresh = () => client.invalidateQueries({ queryKey: getListServicesQueryKey() })
  const createService = useCreateService({
    mutation: { onSuccess: (service) => setEditing(service) },
  })

  const columns: DataListColumn<Service>[] = [
    {
      id: 'name',
      header: t('field.name'),
      primary: true,
      cell: (row) => (
        <span className="inline-flex items-center gap-1.5">
          {row.got_number ? (
            <span className="numeric rounded bg-sunken px-1 text-[0.625rem] font-bold text-ink-soft">
              {t('services.got')} {row.got_number}
            </span>
          ) : null}
          {row.name ?? '—'}
          {row.travel_expenses ? (
            <Route className="size-3.5 text-rust" aria-label={t('services.travelExpenses')} />
          ) : null}
          {row.hidden ? (
            <EyeOff className="size-3.5 text-ink-faint" aria-label={t('services.hidden')} />
          ) : null}
        </span>
      ),
    },
    {
      id: 'factor',
      header: t('field.factor'),
      numeric: true,
      cell: (row) => percent(row.factor),
    },
    { id: 'vat', header: t('field.vat'), numeric: true, cell: (row) => percent(row.vat_percent) },
    {
      id: 'price',
      header: t('field.price'),
      numeric: true,
      cell: (row) => money(row.gross_price),
    },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('services.title')}
        actions={
          <Button
            variant="accent"
            onClick={() => createService.mutate({ data: { type: 'self_defined' } })}
          >
            <Plus className="size-4" />
            {t('services.new')}
          </Button>
        }
      />

      <div className="mt-4 flex flex-wrap items-center gap-4">
        <input
          type="search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t('services.searchPlaceholder')}
          className="w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9 sm:max-w-sm"
        />
        <CheckboxField
          label={t('services.showHidden')}
          checked={showHidden}
          onChange={(event) => setShowHidden(event.target.checked)}
        />
        <CheckboxField
          label={t('record.showArchived')}
          checked={showArchived}
          onChange={(event) => setShowArchived(event.target.checked)}
        />
      </div>

      <DataList
        className="mt-4"
        data={services.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={services.isPending}
        error={services.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) => setEditing(row)}
      />

      {editing ? (
        <ServiceDialog
          service={editing}
          onClose={() => {
            setEditing(null)
            void refresh()
          }}
        />
      ) : null}
    </div>
  )
}

/**
 * Editing a service is a small, focused job, so it happens in a dialog rather than on its
 * own page — the vet stays in the list they were searching.
 */
function ServiceDialog({ service, onClose }: { service: Service; onClose: () => void }) {
  const { t } = useTranslation()
  const { money } = useLocaleFormat()
  const client = useQueryClient()
  const [current, setCurrent] = useState(service)
  const patchService = usePatchService({
    mutation: {
      onSuccess: (updated) => {
        setCurrent(updated)
        void client.invalidateQueries({ queryKey: getListServicesQueryKey() })
      },
    },
  })
  const patch = (data: Parameters<typeof patchService.mutate>[0]['data']) =>
    patchService.mutate({ id: service.id, data })

  const isGot = current.type === 'got'

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && onClose()}
      title={isGot ? `${t('services.got')} ${current.got_number ?? ''}` : t('services.new')}
      description={isGot ? t('services.gotHint') : undefined}
      footer={
        <Button variant="primary" onClick={onClose}>
          {t('action.close')}
        </Button>
      }
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <TextField
          label={t('field.name')}
          defaultValue={current.name ?? ''}
          wrapperClassName="sm:col-span-2"
          onBlur={(event) =>
            event.target.value !== (current.name ?? '') && patch({ name: event.target.value })
          }
        />
        {isGot ? (
          <TextField
            label={t('field.gotNumber')}
            defaultValue={current.got_number ?? ''}
            onBlur={(event) =>
              event.target.value !== (current.got_number ?? '') &&
              patch({ got_number: event.target.value })
            }
          />
        ) : null}
        <NumberInput
          label={t('field.factor')}
          value={current.factor ?? null}
          decimals={3}
          onChange={(value) => value && patch({ factor: value })}
        />
        <SelectField
          label={t('field.vat')}
          defaultValue={current.vat_percent ?? ''}
          onChange={(event) => patch({ vat_percent: event.target.value || null })}
        >
          <option value="">—</option>
          <option value="19.000">19 %</option>
          <option value="7.000">7 %</option>
        </SelectField>
        {/* The GOT publishes net fees, so that is the figure edited here; the gross beneath
            it is what the customer will be billed. */}
        <NumberInput
          label={t('field.priceNet')}
          value={current.net_price ?? null}
          hint={
            current.gross_price
              ? `${t('field.priceGross')}: ${money(current.gross_price)}`
              : undefined
          }
          onChange={(value) => value && patch({ net_price: value })}
        />

        <div className="flex flex-col gap-1 sm:col-span-2">
          <CheckboxField
            label={t('services.travelExpenses')}
            defaultChecked={current.travel_expenses}
            onChange={(event) => patch({ travel_expenses: event.target.checked })}
          />
          <p className="text-xs text-ink-faint">{t('services.travelExpensesHint')}</p>
          <CheckboxField
            label={t('services.hidden')}
            defaultChecked={current.hidden}
            onChange={(event) => patch({ hidden: event.target.checked })}
          />
        </div>

        <span className="sm:col-span-2">
          <IncompleteBadge missing={current.missing_fields} />
        </span>
      </div>
    </Dialog>
  )
}
