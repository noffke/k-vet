import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import {
  getGetManufacturerQueryKey,
  getGetSupplierQueryKey,
  getListManufacturersQueryKey,
  getListSuppliersQueryKey,
  useArchiveManufacturer,
  useArchiveSupplier,
  useGetManufacturer,
  useGetSupplier,
  usePatchManufacturer,
  usePatchSupplier,
  useUnarchiveManufacturer,
  useUnarchiveSupplier,
} from '@/api/generated/endpoints'
import type { AddressBookEntry } from '@/api/generated/model'
import { ArchiveButton } from '@/components/ArchiveButton'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { TextField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'

export function ManufacturerDetailPage() {
  const { id } = useParams({ from: '/app/masterdata/manufacturers/$id' })
  return <AddressBookDetail kind="manufacturer" entryId={Number(id)} />
}

export function SupplierDetailPage() {
  const { id } = useParams({ from: '/app/masterdata/suppliers/$id' })
  return <AddressBookDetail kind="supplier" entryId={Number(id)} />
}

/**
 * One Hersteller or Lieferant. The two are separate resources on the server — sqlx checks
 * every statement against its own table — but the record, the form and the rules are the
 * same, so one component serves both and only the request it makes differs.
 */
function AddressBookDetail({
  kind,
  entryId,
}: {
  kind: 'manufacturer' | 'supplier'
  entryId: number
}) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const isManufacturer = kind === 'manufacturer'

  // Both hooks are called, as they must be; only the matching one is allowed to fetch.
  const manufacturer = useGetManufacturer(entryId, { query: { enabled: isManufacturer } })
  const supplier = useGetSupplier(entryId, { query: { enabled: !isManufacturer } })
  const entry = isManufacturer ? manufacturer : supplier

  const patchManufacturer = usePatchManufacturer()
  const patchSupplier = usePatchSupplier()
  const archiveManufacturer = useArchiveManufacturer()
  const archiveSupplier = useArchiveSupplier()
  const unarchiveManufacturer = useUnarchiveManufacturer()
  const unarchiveSupplier = useUnarchiveSupplier()

  const store = (updated: AddressBookEntry) => {
    client.setQueryData(
      isManufacturer ? getGetManufacturerQueryKey(entryId) : getGetSupplierQueryKey(entryId),
      updated,
    )
    void client.invalidateQueries({
      queryKey: isManufacturer ? getListManufacturersQueryKey() : getListSuppliersQueryKey(),
    })
  }

  const autoSave = useAutoSave<AddressBookEntry>({
    save: (patch) => {
      const data = patch as Parameters<typeof patchManufacturer.mutateAsync>[0]['data']
      return isManufacturer
        ? patchManufacturer.mutateAsync({ id: entryId, data })
        : patchSupplier.mutateAsync({ id: entryId, data })
    },
    onSaved: store,
  })

  if (entry.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!entry.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = entry.data

  const archive = isManufacturer ? archiveManufacturer : archiveSupplier
  const unarchive = isManufacturer ? unarchiveManufacturer : unarchiveSupplier
  const newLabel = isManufacturer ? t('masterData.newManufacturer') : t('masterData.newSupplier')

  const field = (key: keyof AddressBookEntry & string, label: string, className?: string) => (
    <TextField
      label={label}
      defaultValue={(record[key] as string | null) ?? ''}
      wrapperClassName={className}
      error={autoSave.fieldErrors[key] ? t(autoSave.fieldErrors[key] ?? '') : undefined}
      onChange={(event) =>
        autoSave.set({ [key]: event.target.value || null } as Partial<AddressBookEntry>)
      }
      onBlur={() => void autoSave.flush()}
    />
  )

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/masterdata" label={t('masterData.title')} />}
        eyebrow={isManufacturer ? t('masterData.manufacturers') : t('masterData.suppliers')}
        title={record.name ?? newLabel}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            <ArchiveButton
              archived={record.archived}
              name={record.name ?? newLabel}
              onArchive={() => archive.mutate({ id: entryId }, { onSuccess: store })}
              onUnarchive={() => unarchive.mutate({ id: entryId }, { onSuccess: store })}
            />
          </>
        }
      >
        <span className="flex gap-1">
          <IncompleteBadge missing={record.missing_fields} />
          <ArchivedBadge archived={record.archived} />
        </span>
      </PageHeader>

      {/* The address is optional and stored field by field, so a half-typed one is fine. */}
      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-[1fr_2fr]">
        {field('name', t('field.name'), 'sm:col-span-2')}
        {field('addr_addon', t('field.addon'), 'sm:col-span-2')}
        {field('addr_street', t('field.street'), 'sm:col-span-2')}
        {field('addr_zip', t('field.zip'))}
        {field('addr_city', t('field.city'))}
      </section>
    </div>
  )
}
