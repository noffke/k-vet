import { AlertCircle, Check, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { SaveState } from '@/lib/autosave'
import { cn } from '@/lib/utils'

interface SaveIndicatorProps {
  state: SaveState
  /** Shown when a save failed for a non-field reason. */
  error?: string | null
  onRetry?: (() => void) | undefined
  className?: string | undefined
}

/**
 * The only feedback the app gives about saving — there is no save button, so this is
 * where the vet looks to know their text is stored.
 */
export function SaveIndicator({ state, error, onRetry, className }: SaveIndicatorProps) {
  const { t } = useTranslation()

  if (state === 'idle') {
    return <span className={cn('text-xs text-ink-faint', className)}>{t('save.hint')}</span>
  }

  if (state === 'saving') {
    return (
      <span className={cn('flex items-center gap-1.5 text-xs text-ink-soft', className)}>
        <Loader2 className="size-3.5 animate-spin" aria-hidden />
        {t('save.saving')}
      </span>
    )
  }

  if (state === 'saved') {
    return (
      <span
        className={cn('flex items-center gap-1.5 text-xs text-sage-deep', className)}
        role="status"
      >
        <Check className="size-3.5" aria-hidden />
        {t('save.saved')}
      </span>
    )
  }

  return (
    <span
      className={cn('flex items-center gap-1.5 text-xs font-medium text-danger', className)}
      role="alert"
    >
      <AlertCircle className="size-3.5" aria-hidden />
      {error ?? t('save.failed')}
      {onRetry ? (
        <button type="button" onClick={onRetry} className="underline underline-offset-2">
          {t('save.retry')}
        </button>
      ) : null}
    </span>
  )
}
