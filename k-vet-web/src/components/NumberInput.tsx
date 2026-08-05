import { useEffect, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Field } from '@/components/ui/field'
import { formatNumber } from '@/lib/format'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

export interface NumberInputProps {
  label: string
  /** Wire value: a dot-decimal string, or null when empty. */
  value: string | null | undefined
  /** Called with the new wire value (or null when the field is cleared). */
  onChange: (value: string | null) => void
  /** Decimals used for the wire value — money and quantities use 2, factors 3. */
  decimals?: number
  /**
   * Unit shown inside the field, after the value ("€", "%"). It is decoration, not part
   * of what is typed or stored — the field still holds a bare number.
   */
  unit?: string | undefined
  /** Server-side field error to display. */
  error?: string | undefined
  hint?: string | undefined
  placeholder?: string | undefined
  disabled?: boolean | undefined
  className?: string | undefined
  wrapperClassName?: string | undefined
  onBlur?: (() => void) | undefined
}

/**
 * Numeric input in the active locale's conventions: "1,5" in de-DE, "1.5" in en-US.
 *
 * The typed text stays untouched while the field has focus (so a half-typed number is
 * never rewritten under the cursor); on blur it is re-rendered from the stored value.
 * Invalid input shows an error and is not propagated — the last valid value stays stored,
 * mirroring how the server treats invalid input (research R14).
 */
export function NumberInput({
  label,
  value,
  onChange,
  decimals = 2,
  unit,
  error,
  hint,
  placeholder,
  disabled,
  className,
  wrapperClassName,
  onBlur,
}: NumberInputProps) {
  const { t } = useTranslation()
  const { locale, parseNumber } = useLocaleFormat()
  const unitId = useId()
  const [text, setText] = useState(() => displayValue(value, locale))
  const [focused, setFocused] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)

  // Re-render on external changes (server response, locale switch) unless being edited.
  useEffect(() => {
    if (!focused) setText(displayValue(value, locale))
  }, [value, locale, focused])

  const handleChange = (raw: string) => {
    setText(raw)
    if (raw.trim() === '') {
      setLocalError(null)
      onChange(null)
      return
    }
    const parsed = parseNumber(raw)
    if (parsed === null) {
      setLocalError(t('value.notANumber'))
      return
    }
    setLocalError(null)
    onChange(parsed.toFixed(decimals))
  }

  const shown = localError ?? error

  return (
    <Field label={label} error={shown} hint={hint} className={wrapperClassName}>
      {(id) => (
        <div className="relative">
          <input
            id={id}
            // `text` with a numeric keypad hint: `type=number` would fight the decimal comma.
            type="text"
            inputMode="decimal"
            autoComplete="off"
            value={text}
            placeholder={placeholder}
            disabled={disabled}
            aria-invalid={shown ? true : undefined}
            aria-describedby={unit ? unitId : undefined}
            // Room for the unit: its own inset, its width in digit widths, and a gap.
            style={unit ? { paddingInlineEnd: `calc(1.25rem + ${unit.length}ch)` } : undefined}
            onFocus={() => {
              setFocused(true)
              setText(editValue(value, locale))
            }}
            onBlur={() => {
              setFocused(false)
              setText(displayValue(value, locale))
              setLocalError(null)
              onBlur?.()
            }}
            onChange={(event) => handleChange(event.target.value)}
            className={cn(
              'numeric w-full rounded-control border border-line-strong bg-surface px-3 py-2',
              'text-right text-ink min-h-11 sm:min-h-9 placeholder:text-ink-faint',
              'disabled:bg-sunken disabled:text-ink-faint',
              shown && 'border-danger bg-danger/5',
              className,
            )}
          />
          {unit ? (
            <span
              id={unitId}
              // Decoration only: clicks fall through to the input, and the value stays a
              // bare number. Screen readers get it as the field's description.
              className={cn(
                'numeric pointer-events-none absolute inset-y-0 right-3 flex items-center',
                disabled ? 'text-ink-faint' : 'text-ink-soft',
              )}
            >
              {unit}
            </span>
          ) : null}
        </div>
      )}
    </Field>
  )
}

/** Read-only rendering: grouped and locale-formatted. */
function displayValue(value: string | null | undefined, locale: 'de-DE' | 'en-US'): string {
  if (value === null || value === undefined || value === '') return ''
  return formatNumber(value, locale, { maximumFractionDigits: 3 })
}

/** Editing rendering: no group separators, but the locale's decimal separator. */
function editValue(value: string | null | undefined, locale: 'de-DE' | 'en-US'): string {
  if (value === null || value === undefined || value === '') return ''
  return formatNumber(value, locale, { maximumFractionDigits: 3, useGrouping: false })
}
