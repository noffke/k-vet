import { RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { writesSettled } from '@/api/fetcher'
import { Button } from '@/components/ui/button'
import { useNewVersion } from '@/lib/version'

/**
 * Says that a new k-vet release is running and offers to reload into it.
 *
 * The open page is still the old frontend, talking to the new backend; it keeps working until
 * the vet reloads, which she does when it suits her — the strip stays until then. Reloading
 * must not cost what she just typed: the field she is in is left, so it saves, and the reload
 * waits until every write has come back.
 */
export function UpdateBanner() {
  const { t } = useTranslation()
  const version = useNewVersion()
  const [reloading, setReloading] = useState(false)

  if (!version) return null

  const reload = async () => {
    setReloading(true)
    // Leaving a field is what saves it; Safari does not move the focus to a clicked button.
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
    await writesSettled()
    window.location.reload()
  }

  return (
    <div
      role="status"
      className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 border-b border-line-strong bg-rust-soft px-4 py-1.5 text-sm text-ink print:hidden"
    >
      <span className="font-medium">{t('update.available', { version })}</span>
      <Button
        variant="accent"
        size="small"
        className="min-h-11 sm:min-h-9"
        disabled={reloading}
        onClick={() => void reload()}
      >
        <RefreshCw className="size-4" aria-hidden />
        {reloading ? t('update.saving') : t('update.reload')}
      </Button>
    </div>
  )
}
