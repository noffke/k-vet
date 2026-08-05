import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Field } from '@/components/ui/field'
import { formatTimeOfDay, parseTime } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

export interface TimeInputProps {
  label: string
  /** `HH:mm`, or an empty string when unset. */
  value: string
  onChange: (value: string) => void
  error?: string | undefined
  hint?: string | undefined
  disabled?: boolean | undefined
  wrapperClassName?: string | undefined
  onBlur?: (() => void) | undefined
}

/**
 * Time typed in the active locale and stored as `HH:mm`.
 *
 * This is deliberately not `<input type="time">`: browsers render that control in the
 * *browser's* locale, not the page's, so on an en-US browser the practice's German screen
 * asked for `--:-- --` with an AM/PM box and would not take `15:00` at all. It is the one
 * place the browser locale could leak into an app that otherwise formats everything itself.
 *
 * Input is forgiving in both directions — `1500`, `15`, `15.00` and `3pm` all work — so the
 * en-US habit still types cleanly on a de-DE screen.
 */
export function TimeInput({
  label,
  value,
  onChange,
  error,
  hint,
  disabled,
  wrapperClassName,
  onBlur,
}: TimeInputProps) {
  const { t } = useTranslation()
  const { locale } = useLocaleFormat()
  const [text, setText] = useState(() => formatTimeOfDay(value, locale))
  const [focused, setFocused] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)

  useEffect(() => {
    if (!focused) setText(formatTimeOfDay(value, locale))
  }, [value, locale, focused])

  const handleChange = (raw: string) => {
    setText(raw)
    if (raw.trim() === '') {
      setLocalError(null)
      onChange('')
      return
    }
    const parsed = parseTime(raw)
    if (parsed === null) {
      setLocalError(t('value.notATime'))
      return
    }
    setLocalError(null)
    onChange(parsed)
  }

  const shown = localError ?? error
  const placeholder = locale === 'en-US' ? 'hh:mm am' : 'SS:MM'

  return (
    <Field label={label} error={shown} hint={hint} className={wrapperClassName}>
      {(id) => (
        <input
          id={id}
          type="text"
          inputMode="numeric"
          autoComplete="off"
          value={text}
          placeholder={placeholder}
          disabled={disabled}
          aria-invalid={shown ? true : undefined}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false)
            setText(formatTimeOfDay(value, locale))
            setLocalError(null)
            onBlur?.()
          }}
          onChange={(event) => handleChange(event.target.value)}
          className={cn(
            'numeric w-full rounded-control border border-line-strong bg-surface px-3 py-2',
            'text-ink min-h-11 sm:min-h-9 placeholder:text-ink-faint',
            'disabled:bg-sunken disabled:text-ink-faint',
            shown && 'border-danger bg-danger/5',
          )}
        />
      )}
    </Field>
  )
}
