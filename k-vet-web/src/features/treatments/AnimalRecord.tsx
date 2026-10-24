import { useQueryClient } from '@tanstack/react-query'
import { NotebookPen, Upload } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiFetch } from '@/api/fetcher'
import {
  getDownloadAttachmentUrl,
  getGetPatientTreatmentQueryKey,
  getGetTreatmentQueryKey,
  getListPatientTreatmentFilesQueryKey,
  useGetPatientTreatment,
  useListPatientTreatmentFiles,
  usePatchPatientTreatment,
} from '@/api/generated/endpoints'
import type { Attachment, PatientTreatment, TextBlock } from '@/api/generated/model'
import { TextBlockPicker } from '@/components/TextBlockPicker'
import { Button } from '@/components/ui/button'
import { RichTextField, type RichTextHandle } from '@/components/ui/richtext'
import { useAutoSave } from '@/lib/autosave'
import { useLocaleFormat } from '@/lib/locale'
import type { SaveStatus } from '@/lib/saveState'

/** Which of the two texts the picker was opened for. */
type TextField = 'treatment_reason' | 'finding'

/**
 * One animal's record of one visit: why it was seen, what was found, and the files that belong
 * to it — what the vet writes in during the examination, in the animal's tab.
 *
 * It auto-saves on its own, but has no indicator of its own: it reports its state, and the
 * page shows one for everything on it.
 */
export function AnimalRecord({
  recordId,
  onSaveState,
}: {
  recordId: number
  onSaveState: (status: SaveStatus) => void
}) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const { date } = useLocaleFormat()
  const fileInput = useRef<HTMLInputElement>(null)

  // The two editors are uncontrolled, so a block is inserted through the editor itself. It
  // keeps its selection while the picker has the focus, so the block lands where the caret was
  // (issues.md 11).
  const reasonEditor = useRef<RichTextHandle>(null)
  const findingEditor = useRef<RichTextHandle>(null)
  const [pickerFor, setPickerFor] = useState<TextField | null>(null)

  const record = useGetPatientTreatment(recordId)
  const files = useListPatientTreatmentFiles(recordId)
  const treatmentId = record.data?.treatment_id
  const patchRecord = usePatchPatientTreatment()

  const autoSave = useAutoSave<PatientTreatment>({
    save: (patch) =>
      patchRecord.mutateAsync({
        id: recordId,
        data: patch as Parameters<typeof patchRecord.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => {
      client.setQueryData(getGetPatientTreatmentQueryKey(recordId), updated)
      // The appointment's customer lock counts a reason or finding as entered content.
      if (treatmentId) {
        void client.invalidateQueries({ queryKey: getGetTreatmentQueryKey(treatmentId) })
      }
    },
  })

  useEffect(() => {
    onSaveState({ state: autoSave.state, error: autoSave.error })
  }, [autoSave.state, autoSave.error, onSaveState])

  if (record.isPending) return <p className="mt-4 text-sm text-ink-faint">{t('list.loading')}</p>
  if (!record.data) return <p className="mt-4 text-sm text-danger">{t('error.notFound')}</p>
  const current = record.data

  /**
   * Puts a block in at the caret, or at the end when the field was never focused, as
   * paragraphs of their own with the caret on the line after them (issues.md 3). The editor
   * reports the change like anything typed; it is saved at once rather than after the pause.
   */
  const insertBlock = (field: TextField, block: TextBlock) => {
    setPickerFor(null)
    if (!block.content) return
    const editor = field === 'treatment_reason' ? reasonEditor.current : findingEditor.current
    editor?.insert(block.content)
    void autoSave.flush()
  }

  const upload = async (file: File) => {
    const form = new FormData()
    form.append('file', file)
    form.append('kind', 'treatment_file')
    form.append('patient_treatment_id', String(recordId))
    await apiFetch<Attachment>('/api/attachments', { method: 'POST', body: form })
    await client.invalidateQueries({
      queryKey: getListPatientTreatmentFilesQueryKey(recordId),
    })
  }

  return (
    <>
      <section className="mt-4 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
        <div>
          <RichTextField
            ref={reasonEditor}
            label={t('field.treatmentReason')}
            defaultValue={current.treatment_reason}
            onChange={(treatment_reason) => autoSave.set({ treatment_reason })}
            onBlur={() => void autoSave.flush()}
          />
          <Button size="small" className="mt-1.5" onClick={() => setPickerFor('treatment_reason')}>
            <NotebookPen className="size-4" />
            {t('textBlocks.insert')}
          </Button>
        </div>
        <div>
          <RichTextField
            ref={findingEditor}
            label={t('field.finding')}
            defaultValue={current.finding}
            onChange={(finding) => autoSave.set({ finding })}
            onBlur={() => void autoSave.flush()}
          />
          <Button size="small" className="mt-1.5" onClick={() => setPickerFor('finding')}>
            <NotebookPen className="size-4" />
            {t('textBlocks.insert')}
          </Button>
        </div>
      </section>

      <TextBlockPicker
        open={pickerFor !== null}
        onOpenChange={(open) => {
          if (!open) setPickerFor(null)
        }}
        onPick={(block) => {
          if (pickerFor) insertBlock(pickerFor, block)
        }}
      />

      <section className="mt-5">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-sm font-semibold text-ink">{t('patients.files')}</h3>
          <Button size="small" onClick={() => fileInput.current?.click()}>
            <Upload className="size-4" />
            {t('action.upload')}
          </Button>
        </div>
        <input
          ref={fileInput}
          type="file"
          className="hidden"
          onChange={async (event) => {
            const file = event.target.files?.[0]
            if (file) await upload(file)
          }}
        />
        <ul className="mt-2 flex flex-col gap-2">
          {(files.data ?? []).map((file) => (
            <li
              key={file.id}
              className="flex items-center justify-between gap-3 rounded-card border border-line bg-surface px-3 py-2 text-sm"
            >
              {/* A new tab, so opening a scan does not navigate away from the record the vet
                  is in the middle of writing. */}
              <a
                href={getDownloadAttachmentUrl(file.id)}
                target="_blank"
                rel="noreferrer"
                className="text-rust hover:underline"
              >
                {file.orig_name}
              </a>
              <span className="numeric text-xs text-ink-faint">{date(file.created_at)}</span>
            </li>
          ))}
          {(files.data ?? []).length === 0 ? (
            <li className="rounded-card border border-dashed border-line-strong px-4 py-4 text-center text-sm text-ink-faint">
              {t('list.empty')}
            </li>
          ) : null}
        </ul>
      </section>
    </>
  )
}
