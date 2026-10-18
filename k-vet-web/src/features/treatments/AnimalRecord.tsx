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
import { autoGrow, TextAreaField } from '@/components/ui/field'
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

  // The two textareas are uncontrolled (`defaultValue`), so inserting is done on the element
  // itself. The caret has to be remembered as it moves: opening the picker takes focus away,
  // and by the time a block is chosen the selection is gone (issues.md 11).
  const reasonInput = useRef<HTMLTextAreaElement>(null)
  const findingInput = useRef<HTMLTextAreaElement>(null)
  const caret = useRef<Record<TextField, number | null>>({ treatment_reason: null, finding: null })
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
   * Drops a block's text in at the caret, or at the end when the field was never focused.
   *
   * Written straight onto the element and then handed to auto-save: the field is uncontrolled,
   * so React will not re-render it from state, and an `onChange` is not fired by assigning
   * `value`.
   */
  const insertBlock = (field: TextField, block: TextBlock) => {
    const element = field === 'treatment_reason' ? reasonInput.current : findingInput.current
    const text = block.content ?? ''
    setPickerFor(null)
    if (!element || text === '') return

    const at = caret.current[field] ?? element.value.length
    const before = element.value.slice(0, at)
    const after = element.value.slice(at)
    // A block is a paragraph, not a word: separate it from text it lands next to, on both
    // sides (issues.md 3). The trailing newline is what the vet asked for — after inserting,
    // the caret sits on a fresh line, ready for the next block or for typing.
    const lead = before !== '' && !before.endsWith('\n') ? '\n' : ''
    const trail = text.endsWith('\n') ? '' : '\n'
    const insert = lead + text + trail
    element.value = before + insert + after

    const caretAfter = at + insert.length
    caret.current[field] = caretAfter
    element.focus()
    element.setSelectionRange(caretAfter, caretAfter)
    // Assigning `value` fires no input event, so the field would keep the height it had and
    // hide the block that was just put in it.
    autoGrow(element)

    autoSave.set({ [field]: element.value || null })
    void autoSave.flush()
  }

  /** Remembers where the caret is, for as long as the field still knows. */
  const rememberCaret = (field: TextField) => (event: { currentTarget: HTMLTextAreaElement }) => {
    caret.current[field] = event.currentTarget.selectionStart
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
          <TextAreaField
            ref={reasonInput}
            label={t('field.treatmentReason')}
            defaultValue={current.treatment_reason ?? ''}
            onChange={(event) => autoSave.set({ treatment_reason: event.target.value || null })}
            onSelect={rememberCaret('treatment_reason')}
            onBlur={(event) => {
              rememberCaret('treatment_reason')(event)
              void autoSave.flush()
            }}
          />
          <Button size="small" className="mt-1.5" onClick={() => setPickerFor('treatment_reason')}>
            <NotebookPen className="size-4" />
            {t('textBlocks.insert')}
          </Button>
        </div>
        <div>
          <TextAreaField
            ref={findingInput}
            label={t('field.finding')}
            defaultValue={current.finding ?? ''}
            onChange={(event) => autoSave.set({ finding: event.target.value || null })}
            onSelect={rememberCaret('finding')}
            onBlur={(event) => {
              rememberCaret('finding')(event)
              void autoSave.flush()
            }}
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
