import { CalendarDays } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Field } from '@/components/ui/field'
import { formatDate } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

export interface DateInputProps {
  label: string
  /** ISO date (`yyyy-MM-dd`) or null. */
  value: string | null | undefined
  onChange: (value: string | null) => void
  error?: string | undefined
  hint?: string | undefined
  disabled?: boolean | undefined
  wrapperClassName?: string | undefined
}

/**
 * Date input typed in the active locale (`04.05.2026` in de-DE, `5/4/2026` in en-US) and
 * stored as an ISO date. Invalid input is shown with an error and not propagated.
 *
 * Typing is the fast path — short forms like `4.5.26` are accepted and expanded on blur —
 * and the calendar button opens the platform's own picker for the times when the vet is
 * looking for a weekday rather than a date.
 */
export function DateInput({
  label,
  value,
  onChange,
  error,
  hint,
  disabled,
  wrapperClassName,
}: DateInputProps) {
  const { t } = useTranslation()
  const { locale, parseDate } = useLocaleFormat()
  const [text, setText] = useState(() => formatDate(value, locale))
  const [focused, setFocused] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)
  const picker = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!focused) setText(formatDate(value, locale))
  }, [value, locale, focused])

  const handleChange = (raw: string) => {
    setText(raw)
    if (raw.trim() === '') {
      setLocalError(null)
      onChange(null)
      return
    }
    const iso = parseDate(raw)
    if (iso === null) {
      setLocalError(t('value.notADate'))
      return
    }
    setLocalError(null)
    onChange(iso)
  }

  const shown = localError ?? error
  const placeholder = locale === 'en-US' ? 'M/D/YYYY' : 'TT.MM.JJJJ'

  return (
    <Field label={label} error={shown} hint={hint} className={wrapperClassName}>
      {(id) => (
        <div className="relative">
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
              setText(formatDate(value, locale))
              setLocalError(null)
            }}
            onChange={(event) => handleChange(event.target.value)}
            className={cn(
              'numeric w-full rounded-control border border-line-strong bg-surface px-3 py-2 pr-11',
              'text-ink min-h-11 sm:min-h-9 placeholder:text-ink-faint',
              'disabled:bg-sunken disabled:text-ink-faint',
              shown && 'border-danger bg-danger/5',
            )}
          />
          {/* The native picker, borrowed for its calendar only: it stays out of the layout,
              so the date itself is always read and written in the active locale's format.
              `sr-only` keeps it rendered — `showPicker()` refuses on a hidden input. */}
          <input
            ref={picker}
            type="date"
            tabIndex={-1}
            aria-hidden
            value={value ?? ''}
            onChange={(event) => onChange(event.target.value || null)}
            className="sr-only"
          />
          <button
            type="button"
            disabled={disabled}
            aria-label={t('action.openCalendar')}
            onClick={() => picker.current?.showPicker()}
            className={cn(
              'absolute inset-y-0 right-0 flex w-11 items-center justify-center rounded-r-control',
              'text-ink-faint hover:text-ink disabled:pointer-events-none',
            )}
          >
            <CalendarDays className="size-4" />
          </button>
        </div>
      )}
    </Field>
  )
}
