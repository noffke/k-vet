import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { Archive, ArchiveRestore, EyeOff, Plus, Route } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetServiceQueryKey,
  getListServicesQueryKey,
  useArchiveService,
  useCreateService,
  useGetService,
  useListServices,
  usePatchService,
  useUnarchiveService,
} from '@/api/generated/endpoints'
import type { Service } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { DataList, type DataListColumn } from '@/components/DataList'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { CheckboxField, TextField } from '@/components/ui/field'
import { VatSelect } from '@/components/VatSelect'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'

/**
 * The service catalog: the imported GOT 2022 schedule plus the practice's own positions.
 *
 * The catalog has ~900 GOT entries, so this screen is search-first. Surgical positions are
 * hidden until the toggle asks for them (FR-021).
 */
export function ServicesPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const { money, percent } = useLocaleFormat()
  const [search, setSearch] = useState('')
  const [showHidden, setShowHidden] = useState(false)
  const [showArchived, setShowArchived] = useState(false)

  const services = useListServices({
    q: search || undefined,
    hidden: showHidden || undefined,
    archived: showArchived || undefined,
  })
  const open = (id: number) => navigate({ to: '/services/$id', params: { id: String(id) } })
  const createService = useCreateService({
    mutation: {
      onSuccess: async (service) => {
        await client.invalidateQueries({ queryKey: getListServicesQueryKey() })
        await open(service.id)
      },
    },
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
        onRowClick={(row) => void open(row.id)}
      />
    </div>
  )
}

/**
 * One service. A GOT position may be renamed and repriced like any other — the practice
 * owns its copy of the schedule — but its number and factor stay mandatory (FR-021).
 */
export function ServiceDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/services/$id' })
  const serviceId = Number(id)
  const client = useQueryClient()
  const { money, currencySymbol } = useLocaleFormat()

  const service = useGetService(serviceId)
  const patchService = usePatchService()
  const archive = useArchiveService()
  const unarchive = useUnarchiveService()

  const store = (updated: Service) => {
    client.setQueryData(getGetServiceQueryKey(serviceId), updated)
    void client.invalidateQueries({ queryKey: getListServicesQueryKey() })
  }

  const autoSave = useAutoSave<Service>({
    save: (patch) =>
      patchService.mutateAsync({
        id: serviceId,
        data: patch as Parameters<typeof patchService.mutateAsync>[0]['data'],
      }),
    onSaved: store,
  })

  if (service.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!service.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = service.data
  const isGot = record.type === 'got'
  const errorFor = (field: string) =>
    autoSave.fieldErrors[field] ? t(autoSave.fieldErrors[field] ?? '') : undefined

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/services" label={t('services.title')} />}
        eyebrow={isGot ? `${t('services.got')} ${record.got_number ?? ''}` : undefined}
        title={record.name ?? t('services.new')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            {record.archived ? (
              <Button onClick={() => unarchive.mutate({ id: serviceId }, { onSuccess: store })}>
                <ArchiveRestore className="size-4" />
                <span className="sr-only sm:not-sr-only">{t('record.unarchive')}</span>
              </Button>
            ) : (
              <Button onClick={() => archive.mutate({ id: serviceId }, { onSuccess: store })}>
                <Archive className="size-4" />
                <span className="sr-only sm:not-sr-only">{t('record.archive')}</span>
              </Button>
            )}
          </>
        }
      >
        <span className="flex gap-1">
          <IncompleteBadge missing={record.missing_fields} />
          <ArchivedBadge archived={record.archived} />
        </span>
      </PageHeader>

      {isGot ? <p className="mt-3 text-xs text-ink-faint">{t('services.gotHint')}</p> : null}

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <TextField
          label={t('field.name')}
          defaultValue={record.name ?? ''}
          wrapperClassName="sm:col-span-2"
          error={errorFor('name')}
          onChange={(event) => autoSave.set({ name: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        {isGot ? (
          <TextField
            label={t('field.gotNumber')}
            defaultValue={record.got_number ?? ''}
            error={errorFor('got_number')}
            onChange={(event) => autoSave.set({ got_number: event.target.value || null })}
            onBlur={() => void autoSave.flush()}
          />
        ) : null}
        <NumberInput
          label={t('field.factor')}
          value={record.factor ?? null}
          decimals={3}
          unit="%"
          error={errorFor('factor')}
          onChange={(value) => autoSave.set({ factor: value })}
          onBlur={() => void autoSave.flush()}
        />
        <VatSelect
          value={record.vat_percent}
          error={errorFor('vat_percent')}
          onChange={(value) => {
            autoSave.set({ vat_percent: value })
            void autoSave.flush()
          }}
        />
        {/* The GOT publishes net fees, so that is the figure edited here; the gross beneath
            it is what the customer will be billed. */}
        <NumberInput
          label={t('field.priceNet')}
          value={record.net_price ?? null}
          unit={currencySymbol}
          error={errorFor('net_price')}
          hint={
            record.gross_price
              ? `${t('field.priceGross')}: ${money(record.gross_price)}`
              : undefined
          }
          onChange={(value) => autoSave.set({ net_price: value })}
          onBlur={() => void autoSave.flush()}
        />

        <div className="flex flex-col gap-1 sm:col-span-2">
          <CheckboxField
            label={t('services.travelExpenses')}
            defaultChecked={record.travel_expenses}
            onChange={(event) => {
              autoSave.set({ travel_expenses: event.target.checked })
              void autoSave.flush()
            }}
          />
          <p className="text-xs text-ink-faint">{t('services.travelExpensesHint')}</p>
          <CheckboxField
            label={t('services.hidden')}
            defaultChecked={record.hidden}
            onChange={(event) => {
              autoSave.set({ hidden: event.target.checked })
              void autoSave.flush()
            }}
          />
        </div>
      </section>
    </div>
  )
}
