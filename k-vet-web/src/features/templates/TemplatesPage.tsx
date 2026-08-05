import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import {
  Archive,
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  ChevronUp,
  Pill,
  Plus,
  Stethoscope,
  Trash2,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetTemplateQueryKey,
  getListTemplateItemsQueryKey,
  getListTemplatesQueryKey,
  useArchiveTemplate,
  useCreateTemplate,
  useCreateTemplateItem,
  useDeleteTemplateItem,
  useGetTemplate,
  useListTemplateItems,
  useListTemplates,
  useMoveTemplateItem,
  usePatchTemplate,
  usePatchTemplateItem,
} from '@/api/generated/endpoints'
import type { MoveDirection, PickerItem, Template } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { DataList, type DataListColumn } from '@/components/DataList'
import { ItemPicker } from '@/components/ItemPicker'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'

/** The list of reusable line sets. */
export function TemplatesPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [search, setSearch] = useState('')

  const templates = useListTemplates({ q: search || undefined })
  const createTemplate = useCreateTemplate({
    mutation: {
      onSuccess: async (template) => {
        await client.invalidateQueries({ queryKey: getListTemplatesQueryKey() })
        await navigate({ to: '/templates/$id', params: { id: String(template.id) } })
      },
    },
  })

  const columns: DataListColumn<Template>[] = [
    { id: 'name', header: t('field.name'), primary: true, cell: (row) => row.name ?? '—' },
    {
      id: 'items',
      header: t('templates.items'),
      numeric: true,
      cell: (row) => row.item_count,
    },
  ]

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('templates.title')}
        actions={
          <Button variant="accent" onClick={() => createTemplate.mutate()}>
            <Plus className="size-4" />
            {t('templates.new')}
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
        data={templates.data ?? []}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={templates.isPending}
        error={templates.isError ? t('list.error') : null}
        rowBadge={(row) => (
          <>
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        onRowClick={(row) =>
          void navigate({ to: '/templates/$id', params: { id: String(row.id) } })
        }
      />
    </div>
  )
}

/**
 * One template: a name and an ordered list of lines. A template stores references and
 * quantities — prices are pinned when it is applied to a treatment (FR-024).
 */
export function TemplateDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/templates/$id' })
  const templateId = Number(id)
  const client = useQueryClient()
  const navigate = useNavigate()
  const { quantity: formatQuantity } = useLocaleFormat()

  const template = useGetTemplate(templateId)
  const items = useListTemplateItems(templateId)
  const patchTemplate = usePatchTemplate()
  const archive = useArchiveTemplate()

  const refreshItems = () =>
    client.invalidateQueries({ queryKey: getListTemplateItemsQueryKey(templateId) })
  const refreshTemplate = () =>
    client.invalidateQueries({ queryKey: getGetTemplateQueryKey(templateId) })
  const afterItemChange = async () => {
    await refreshItems()
    await refreshTemplate()
    await client.invalidateQueries({ queryKey: getListTemplatesQueryKey() })
  }

  const addItem = useCreateTemplateItem({ mutation: { onSuccess: afterItemChange } })
  const patchItem = usePatchTemplateItem({ mutation: { onSuccess: afterItemChange } })
  const moveItem = useMoveTemplateItem({ mutation: { onSuccess: afterItemChange } })
  const deleteItem = useDeleteTemplateItem({ mutation: { onSuccess: afterItemChange } })

  const autoSave = useAutoSave<Template>({
    save: (patch) =>
      patchTemplate.mutateAsync({
        id: templateId,
        data: patch as Parameters<typeof patchTemplate.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => {
      client.setQueryData(getGetTemplateQueryKey(templateId), updated)
      void client.invalidateQueries({ queryKey: getListTemplatesQueryKey() })
    },
  })

  if (template.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!template.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = template.data
  const lines = items.data ?? []

  const pick = (item: PickerItem) =>
    addItem.mutate({
      id: templateId,
      data:
        item.kind === 'drug_packaging'
          ? { kind: 'drug_packaging', drug_packaging_id: item.id, quantity: '1' }
          : { kind: 'service', service_id: item.id, quantity: '1' },
    })

  const move = (itemId: number, direction: MoveDirection) =>
    moveItem.mutate({ id: itemId, data: { direction } })

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/templates" label={t('templates.title')} />}
        title={record.name ?? t('templates.new')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            <Button
              onClick={() =>
                archive.mutate(
                  { id: templateId },
                  { onSuccess: () => void navigate({ to: '/templates' }) },
                )
              }
            >
              <Archive className="size-4" />
              <span className="sr-only sm:not-sr-only">{t('record.archive')}</span>
            </Button>
          </>
        }
      >
        <IncompleteBadge missing={record.missing_fields} />
      </PageHeader>

      <div className="mt-5 rounded-card border border-line bg-surface p-4">
        <TextField
          label={t('field.name')}
          defaultValue={record.name ?? ''}
          onChange={(event) => autoSave.set({ name: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
      </div>

      <section className="mt-6">
        <h2 className="text-base">{t('templates.items')}</h2>
        <div className="mt-3">
          <ItemPicker onPick={pick} disabled={addItem.isPending} />
        </div>

        <ul className="mt-3 flex flex-col gap-2">
          {lines.map((item, index) => (
            <li
              key={item.id}
              className="flex flex-wrap items-center gap-3 rounded-card border border-line bg-surface px-3 py-2.5"
            >
              <span className="numeric text-xs text-ink-faint">{item.position}</span>
              {item.kind === 'drug_packaging' ? (
                <Pill className="size-4 shrink-0 text-rust" aria-hidden />
              ) : (
                <Stethoscope className="size-4 shrink-0 text-ink-soft" aria-hidden />
              )}
              <span className="min-w-0 flex-1 truncate text-sm text-ink">{item.name}</span>
              <span className="w-24">
                <NumberInput
                  label={t('field.quantity')}
                  value={item.quantity}
                  onChange={(value) =>
                    value && patchItem.mutate({ id: item.id, data: { quantity: value } })
                  }
                />
              </span>
              <span className="numeric text-xs text-ink-faint">
                {item.unit ? formatQuantity(item.quantity) : ''} {item.unit ?? ''}
              </span>
              <span className="flex gap-1">
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveTop')}
                  disabled={index === 0}
                  onClick={() => move(item.id, 'top')}
                >
                  <ChevronsUp className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveUp')}
                  disabled={index === 0}
                  onClick={() => move(item.id, 'up')}
                >
                  <ChevronUp className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveDown')}
                  disabled={index === lines.length - 1}
                  onClick={() => move(item.id, 'down')}
                >
                  <ChevronDown className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.moveBottom')}
                  disabled={index === lines.length - 1}
                  onClick={() => move(item.id, 'bottom')}
                >
                  <ChevronsDown className="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('action.delete')}
                  onClick={() => deleteItem.mutate({ id: item.id })}
                >
                  <Trash2 className="size-4 text-danger" />
                </Button>
              </span>
            </li>
          ))}
          {lines.length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>
    </div>
  )
}
