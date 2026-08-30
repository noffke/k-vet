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
 * Every state occupies the same box, so switching between them cannot move the page.
 *
 * This is not cosmetic. The indicator lives in the `PageHeader` actions area, which wraps below
 * the title when it is wide and sits beside it when it is narrow. The idle hint used to be much
 * wider than "Gespeichert", so the first save flipped that wrap decision and shifted everything
 * below the header up by a line — and a tap already in flight (the vet typing, then reaching for
 * the next control) landed on nothing, because `pointerup` missed the element `pointerdown` had
 * hit. One reserved width removes the whole class of lost first taps on a phone.
 */
const shell =
  'inline-flex min-w-[7.5rem] items-center justify-end gap-1.5 whitespace-nowrap text-xs'

/**
 * The only feedback the app gives about saving — there is no save button, so this is
 * where the vet looks to know their text is stored.
 */
export function SaveIndicator({ state, error, onRetry, className }: SaveIndicatorProps) {
  const { t } = useTranslation()

  if (state === 'idle') {
    return <span className={cn(shell, 'text-ink-faint', className)}>{t('save.hint')}</span>
  }

  if (state === 'saving') {
    return (
      <span className={cn(shell, 'text-ink-soft', className)}>
        <Loader2 className="size-3.5 animate-spin" aria-hidden />
        {t('save.saving')}
      </span>
    )
  }

  if (state === 'saved') {
    return (
      <span className={cn(shell, 'text-sage-deep', className)} role="status">
        <Check className="size-3.5" aria-hidden />
        {t('save.saved')}
      </span>
    )
  }

  return (
    <span className={cn(shell, 'font-medium text-danger', className)} role="alert">
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
