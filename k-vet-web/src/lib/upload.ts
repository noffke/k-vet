import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ApiError, apiFetch } from '@/api/fetcher'
import type { Attachment } from '@/api/generated/model'
import { useOperatorConfig } from '@/lib/config'

/**
 * Uploads a file to `/api/attachments` and says what happened: "wird hochgeladen …" while it
 * goes — a photo from a phone takes a moment on a mobile connection — then that it arrived, or
 * why not. A file over `[storage] max_upload_mb` is refused here, before it is sent. Resolves
 * to the stored attachment, or `null` when there is none; the caller never sees a rejection.
 */
export function useUpload() {
  const { t } = useTranslation()
  const { max_upload_mb: maxMb } = useOperatorConfig()

  return async (file: File, parts: Record<string, string> = {}): Promise<Attachment | null> => {
    const name = file.name
    if (file.size > maxMb * 1024 * 1024) {
      toast.error(t('upload.tooLarge', { name, mb: maxMb }))
      return null
    }
    const pending = toast.loading(t('upload.pending', { name }))
    const form = new FormData()
    form.append('file', file)
    for (const [key, value] of Object.entries(parts)) form.append(key, value)
    try {
      const attachment = await apiFetch<Attachment>('/api/attachments', {
        method: 'POST',
        body: form,
      })
      toast.success(t('upload.done', { name }), { id: pending })
      return attachment
    } catch (error) {
      const tooLarge = error instanceof ApiError && error.status === 413
      toast.error(
        tooLarge ? t('upload.tooLarge', { name, mb: maxMb }) : t('upload.failed', { name }),
        { id: pending },
      )
      return null
    }
  }
}
