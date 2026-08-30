import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { Archive, ArchiveRestore } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  getListPackagingsQueryKey,
  useArchiveDrug,
  useGetDrug,
  useListManufacturers,
  useListPackagings,
  useListSuppliers,
  usePatchDrug,
  useUnarchiveDrug,
} from '@/api/generated/endpoints'
import type { Drug } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { CheckboxField, SelectField, TextField } from '@/components/ui/field'
import { VatSelect } from '@/components/VatSelect'
import { PackagingEditor } from '@/features/pharmacy/PackagingEditor'
import { StockPanel } from '@/features/pharmacy/StockPanel'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'

/** The regulatory flags of a drug — informational in this version (FR-012). */
const FLAGS = [
  'submission_receipt',
  'narcotic',
  'vaccine',
  'refrigerate',
  'redesignation',
  // Selects the AMPreisV rule: § 3 Abs. 1 Satz 2 instead of the veterinary bands.
  'human_drug',
] as const

/** One drug: identity, VAT, flags, its packagings and its stock. */
export function DrugDetailPage() {
  const { t } = useTranslation()
  const { money, quantity } = useLocaleFormat()
  const { id } = useParams({ from: '/app/pharmacy/$id' })
  const drugId = Number(id)
  const client = useQueryClient()

  const drug = useGetDrug(drugId)
  const packagings = useListPackagings(drugId)
  const manufacturers = useListManufacturers()
  const suppliers = useListSuppliers()
  const patchDrug = usePatchDrug()
  const archive = useArchiveDrug()
  const unarchive = useUnarchiveDrug()

  const store = (updated: Drug) => {
    client.setQueryData(getGetDrugQueryKey(drugId), updated)
    void client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
  }

  const autoSave = useAutoSave<Drug>({
    save: (patch) =>
      patchDrug.mutateAsync({
        id: drugId,
        data: patch as Parameters<typeof patchDrug.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => {
      store(updated)
      // A changed VAT rate — or the Humanpräparat flag, which picks the AMPreisV rule — moves
      // every computed packaging price. Use the generated key: a hand-written one silently
      // matches nothing, which is why this refresh never actually happened.
      void client.invalidateQueries({ queryKey: getListPackagingsQueryKey(drugId) })
    },
  })

  if (drug.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!drug.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = drug.data
  const original = (packagings.data ?? []).find((packaging) => packaging.kind === 'original')

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/pharmacy" label={t('pharmacy.title')} />}
        title={record.name ?? t('pharmacy.newDrug')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            {record.archived ? (
              <Button onClick={() => unarchive.mutate({ id: drugId }, { onSuccess: store })}>
                <ArchiveRestore className="size-4" />
                <span className="sr-only sm:not-sr-only">{t('record.unarchive')}</span>
              </Button>
            ) : (
              <Button onClick={() => archive.mutate({ id: drugId }, { onSuccess: store })}>
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

      {/*
        The price a customer is quoted, at reading distance (issues.md 6). It is derivable from
        the net field further down, but the vet is usually on the phone when they need it, and a
        hint under an input is not something you find in that moment.
      */}
      {original?.sales_price_gross ? (
        <p className="mt-4 flex flex-wrap items-baseline gap-x-3 gap-y-1 rounded-card border border-line bg-cream-soft px-4 py-3">
          <span className="eyebrow">{t('field.salesPriceGross')}</span>
          <span className="numeric text-2xl font-semibold text-rust">
            {money(original.sales_price_gross)}
          </span>
          {original.quantity && original.unit ? (
            <span className="text-xs text-ink-faint">
              {t('pharmacy.original')} · {quantity(original.quantity)} {original.unit}
            </span>
          ) : null}
        </p>
      ) : null}

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <TextField
          label={t('field.name')}
          defaultValue={record.name ?? ''}
          onChange={(event) => autoSave.set({ name: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <SelectField
          label={t('field.manufacturer')}
          defaultValue={record.manufacturer_id ?? ''}
          onChange={(event) => {
            autoSave.set({
              manufacturer_id: event.target.value ? Number(event.target.value) : null,
            })
            void autoSave.flush()
          }}
        >
          <option value="">—</option>
          {(manufacturers.data ?? []).map((manufacturer) => (
            <option key={manufacturer.id} value={manufacturer.id}>
              {manufacturer.name ?? ''}
            </option>
          ))}
        </SelectField>

        {/* The VAT choices come from the operator's configuration; the rate is pinned per
            drug and again per invoice line. */}
        <VatSelect
          value={record.vat_percent}
          onChange={(value) => {
            autoSave.set({ vat_percent: value })
            void autoSave.flush()
          }}
        />
        <TextField
          label={t('pharmacy.approvalNumber')}
          defaultValue={record.approval_number ?? ''}
          onChange={(event) => autoSave.set({ approval_number: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />

        <fieldset className="border-0 p-0 sm:col-span-2">
          <legend className="eyebrow">{t('pharmacy.flagsLegend')}</legend>
          <div className="mt-1 flex flex-wrap gap-x-5">
            {FLAGS.map((flag) => (
              <CheckboxField
                key={flag}
                label={t(`pharmacy.flags.${camelCase(flag)}`)}
                defaultChecked={record[flag]}
                onChange={(event) => {
                  autoSave.set({ [flag]: event.target.checked } as Partial<Drug>)
                  void autoSave.flush()
                }}
              />
            ))}
          </div>
        </fieldset>
      </section>

      <PackagingEditor
        drug={record}
        packagings={packagings.data ?? []}
        suppliers={suppliers.data ?? []}
      />

      <StockPanel drug={record} original={original} />
    </div>
  )
}

function camelCase(value: string): string {
  return value.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
}
