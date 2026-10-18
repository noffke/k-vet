import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getListTextBlocksQueryKey,
  listTextBlocks,
  useCreateTextBlock,
} from '@/api/generated/endpoints'
import type { TextBlock } from '@/api/generated/model'
import { DataList, type DataListColumn } from '@/components/DataList'
import { LoadMore } from '@/components/LoadMore'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { usePagedList } from '@/lib/paging'

/**
 * The Textbaustein library (issues.md 11): named snippets the vet assembles a Vorbericht or a
 * Therapie from, rather than typing the same three sentences on a phone during every house call.
 */
export function TextBlocksPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [search, setSearch] = useState('')
  const [showArchived, setShowArchived] = useState(false)

  const params = {
    q: search || undefined,
    archived: showArchived || undefined,
  }
  const blocks = usePagedList(getListTextBlocksQueryKey(params), listTextBlocks, params)
  const createBlock = useCreateTextBlock({
    mutation: {
      onSuccess: async (block) => {
        await client.invalidateQueries({ queryKey: getListTextBlocksQueryKey() })
        await navigate({ to: '/text-blocks/$id', params: { id: String(block.id) } })
      },
    },
  })

  const columns: DataListColumn<TextBlock>[] = [
    { id: 'name', header: t('field.name'), primary: true, cell: (row) => row.name ?? '—' },
  ]

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('textBlocks.title')}
        actions={
          <Button variant="accent" onClick={() => createBlock.mutate()}>
            <Plus className="size-4" />
            {t('textBlocks.new')}
          </Button>
        }
      />

      <div className="mt-4 flex flex-wrap items-center gap-4">
        <input
          type="search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t('textBlocks.searchPlaceholder')}
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
        data={blocks.rows}
        columns={columns}
        getRowId={(row) => String(row.id)}
        isLoading={blocks.isPending}
        error={blocks.isError ? t('list.error') : null}
        emptyMessage={t('textBlocks.empty')}
        rowBadge={(row) => (
          <>
            <IncompleteBadge missing={row.missing_fields} />
            <ArchivedBadge archived={row.archived} />
          </>
        )}
        // The text itself, not a column of it: a block runs to several sentences.
        rowSubline={(row) => row.content ?? null}
        onRowClick={(row) =>
          void navigate({ to: '/text-blocks/$id', params: { id: String(row.id) } })
        }
      />
      <LoadMore list={blocks} />
    </div>
  )
}
