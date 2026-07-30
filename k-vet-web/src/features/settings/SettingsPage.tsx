import { useQueryClient } from '@tanstack/react-query'
import { Trash2, Upload } from 'lucide-react'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { apiFetch } from '@/api/fetcher'
import { getGetSettingsQueryKey, useGetSettings, usePatchSettings } from '@/api/generated/endpoints'
import type { Attachment, Settings } from '@/api/generated/model'
import { PageHeader } from '@/components/PageHeader'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { TextAreaField, TextField } from '@/components/ui/field'
import { useAutoSave } from '@/lib/autosave'

/** Textarea text (one address per line) → the array the API stores. */
const parseEmails = (value: string): string[] =>
  value
    .split(/[\n,;]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')

/**
 * Practice settings: what the practice itself owns and changes over the years (FR-037).
 * Everything auto-saves, like every other form in the app. Mail server, invoice number
 * pattern and currency are operator configuration and deliberately absent.
 */
export function SettingsPage() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const fileInput = useRef<HTMLInputElement>(null)

  const settings = useGetSettings()
  const patchSettings = usePatchSettings()

  const autoSave = useAutoSave<Settings>({
    save: (patch) =>
      patchSettings.mutateAsync({
        data: patch as Parameters<typeof patchSettings.mutateAsync>[0]['data'],
      }),
    onSaved: (updated) => client.setQueryData(getGetSettingsQueryKey(), updated),
  })

  if (settings.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!settings.data) return <p className="text-sm text-danger">{t('list.error')}</p>
  const record = settings.data

  const errorFor = (field: keyof Settings & string) =>
    autoSave.fieldErrors[field] ? t(autoSave.fieldErrors[field] ?? '') : undefined

  const uploadLogo = async (file: File) => {
    const form = new FormData()
    form.append('file', file)
    const attachment = await apiFetch<Attachment>('/api/attachments', {
      method: 'POST',
      body: form,
    })
    autoSave.set({ logo_attachment_id: attachment.id })
    await autoSave.flush()
  }

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('settings.title')}
        actions={<SaveIndicator state={autoSave.state} error={autoSave.error} />}
      />

      <section className="mt-5 rounded-card border border-line bg-surface p-4">
        <div className="grid gap-3">
          <TextField
            label={t('settings.practiceName')}
            defaultValue={record.practice_name}
            error={errorFor('practice_name')}
            onChange={(event) => autoSave.set({ practice_name: event.target.value })}
            onBlur={() => void autoSave.flush()}
          />
          <TextField
            label={t('settings.practiceStreet')}
            defaultValue={record.practice_street}
            error={errorFor('practice_street')}
            onChange={(event) => autoSave.set({ practice_street: event.target.value })}
            onBlur={() => void autoSave.flush()}
          />
          <div className="grid gap-3 sm:grid-cols-[1fr_2fr_auto]">
            <TextField
              label={t('settings.practiceZip')}
              defaultValue={record.practice_zip}
              error={errorFor('practice_zip')}
              onChange={(event) => autoSave.set({ practice_zip: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
            <TextField
              label={t('settings.practiceCity')}
              defaultValue={record.practice_city}
              error={errorFor('practice_city')}
              onChange={(event) => autoSave.set({ practice_city: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
            <TextField
              label={t('settings.country')}
              defaultValue={record.practice_country ?? ''}
              error={errorFor('practice_country')}
              onChange={(event) =>
                autoSave.set({ practice_country: event.target.value.toUpperCase() || null })
              }
              onBlur={() => void autoSave.flush()}
            />
          </div>
          <TextField
            label={t('settings.email')}
            hint={t('settings.emailHint')}
            defaultValue={record.email}
            error={errorFor('email')}
            onChange={(event) => autoSave.set({ email: event.target.value })}
            onBlur={() => void autoSave.flush()}
          />
        </div>
      </section>

      <section className="mt-4 rounded-card border border-line bg-surface p-4">
        <h2 className="eyebrow">{t('settings.banking')}</h2>
        <div className="mt-2 grid gap-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <TextField
              label={t('settings.iban')}
              defaultValue={record.iban}
              error={errorFor('iban')}
              onChange={(event) => autoSave.set({ iban: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
            <TextField
              label={t('settings.bic')}
              hint={t('settings.bicHint')}
              defaultValue={record.bic}
              error={errorFor('bic')}
              onChange={(event) => autoSave.set({ bic: event.target.value.toUpperCase() })}
              onBlur={() => void autoSave.flush()}
            />
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <TextField
              label={t('settings.bankName')}
              defaultValue={record.bank_name}
              error={errorFor('bank_name')}
              onChange={(event) => autoSave.set({ bank_name: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
            <TextField
              label={t('settings.ustid')}
              defaultValue={record.ustid}
              error={errorFor('ustid')}
              onChange={(event) => autoSave.set({ ustid: event.target.value })}
              onBlur={() => void autoSave.flush()}
            />
          </div>
        </div>
      </section>

      <section className="mt-4 rounded-card border border-line bg-surface p-4">
        <h2 className="eyebrow">{t('settings.logo')}</h2>
        <div className="mt-2 flex items-center gap-4">
          {record.logo_attachment_id ? (
            <img
              src={`/api/attachments/${record.logo_attachment_id}`}
              alt={t('settings.logo')}
              className="h-12 w-auto rounded border border-line bg-paper p-1"
            />
          ) : (
            <p className="text-sm text-ink-faint">{t('settings.noLogo')}</p>
          )}
          <input
            ref={fileInput}
            type="file"
            accept="image/png,image/jpeg,image/svg+xml"
            className="sr-only"
            onChange={(event) => {
              const file = event.target.files?.[0]
              if (file) void uploadLogo(file)
              event.target.value = ''
            }}
          />
          <Button onClick={() => fileInput.current?.click()}>
            <Upload className="size-4" />
            {t('action.upload')}
          </Button>
          {record.logo_attachment_id ? (
            <Button
              variant="ghost"
              aria-label={t('action.delete')}
              onClick={() => {
                autoSave.set({ logo_attachment_id: null })
                void autoSave.flush()
              }}
            >
              <Trash2 className="size-4 text-danger" />
            </Button>
          ) : null}
        </div>
      </section>

      <section className="mt-4 rounded-card border border-line bg-surface p-4">
        <h2 className="eyebrow">{t('settings.copies')}</h2>
        <div className="mt-2 grid gap-3 sm:grid-cols-2">
          <TextAreaField
            label={t('settings.ccEmails')}
            defaultValue={record.cc_emails.join('\n')}
            rows={3}
            hint={t('settings.emailsHint')}
            error={errorFor('cc_emails')}
            onChange={(event) => autoSave.set({ cc_emails: parseEmails(event.target.value) })}
            onBlur={() => void autoSave.flush()}
          />
          <TextAreaField
            label={t('settings.bccEmails')}
            defaultValue={record.bcc_emails.join('\n')}
            rows={3}
            hint={t('settings.emailsHint')}
            error={errorFor('bcc_emails')}
            onChange={(event) => autoSave.set({ bcc_emails: parseEmails(event.target.value) })}
            onBlur={() => void autoSave.flush()}
          />
        </div>
      </section>
    </div>
  )
}
