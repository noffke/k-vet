import { useQueryClient } from '@tanstack/react-query'
import { useParams } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import {
  getGetTextBlockQueryKey,
  getListTextBlocksQueryKey,
  useArchiveTextBlock,
  useGetTextBlock,
  usePatchTextBlock,
  useUnarchiveTextBlock,
} from '@/api/generated/endpoints'
import type { TextBlock } from '@/api/generated/model'
import { ArchiveButton } from '@/components/ArchiveButton'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { TextAreaField, TextField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'

/** One Textbaustein: what it is called, and the text it inserts. */
export function TextBlockDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/text-blocks/$id' })
  const blockId = Number(id)
  const client = useQueryClient()

  const block = useGetTextBlock(blockId)
  const patchBlock = usePatchTextBlock()

  const store = (updated: TextBlock) => {
    client.setQueryData(getGetTextBlockQueryKey(blockId), updated)
    void client.invalidateQueries({ queryKey: getListTextBlocksQueryKey() })
  }
  const archive = useArchiveTextBlock()
  const unarchive = useUnarchiveTextBlock()

  const autoSave = useAutoSave<TextBlock>({
    save: (patch) =>
      patchBlock.mutateAsync({
        id: blockId,
        data: patch as Parameters<typeof patchBlock.mutateAsync>[0]['data'],
      }),
    onSaved: store,
  })

  if (block.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!block.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = block.data

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/text-blocks" label={t('textBlocks.title')} />}
        title={record.name ?? t('textBlocks.new')}
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            <ArchiveButton
              archived={record.archived}
              name={record.name ?? t('textBlocks.new')}
              onArchive={() => archive.mutate({ id: blockId }, { onSuccess: store })}
              onUnarchive={() => unarchive.mutate({ id: blockId }, { onSuccess: store })}
            />
          </>
        }
      >
        <span className="flex gap-1">
          <IncompleteBadge missing={record.missing_fields} />
          <ArchivedBadge archived={record.archived} />
        </span>
      </PageHeader>

      <section className="mt-5 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
        <TextField
          label={t('field.name')}
          defaultValue={record.name ?? ''}
          error={autoSave.fieldErrors.name ? t(autoSave.fieldErrors.name ?? '') : undefined}
          onChange={(event) => autoSave.set({ name: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <TextAreaField
          label={t('field.content')}
          rows={8}
          defaultValue={record.content ?? ''}
          error={autoSave.fieldErrors.content ? t(autoSave.fieldErrors.content ?? '') : undefined}
          onChange={(event) => autoSave.set({ content: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
      </section>
    </div>
  )
}
