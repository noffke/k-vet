import { Minus, Plus } from 'lucide-react'
import { useEffect, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { currencyDecimals, formatNumber, localiseSeparators } from '@/lib/format'
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
   * An amount of money. It is then always shown with the currency's minor units — `14,80`,
   * never `14,8` — where a quantity drops its trailing zeros.
   */
  money?: boolean
  /**
   * Renders − / + buttons stepping by this much. For the small whole numbers that are
   * counted rather than measured — how many packages arrived — where reaching for the
   * keyboard to change a 1 into a 2 is the slower way round.
   */
  step?: number
  min?: number
  max?: number
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
  decimals,
  money,
  step,
  min,
  max,
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
  const { locale, currency, parseNumber } = useLocaleFormat()
  const unitId = useId()
  const fixed = money ? currencyDecimals(locale, currency) : null
  const wireDecimals = decimals ?? fixed ?? 2
  const [text, setText] = useState(() => displayValue(value, locale, fixed))
  const [focused, setFocused] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)

  // Re-render on external changes (server response, locale switch) unless being edited.
  useEffect(() => {
    if (!focused) setText(displayValue(value, locale, fixed))
  }, [value, locale, fixed, focused])

  const handleChange = (input: string) => {
    // A keypad types a dot wherever it is sold; the field shows the locale's separator.
    const raw = localiseSeparators(input, locale)
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
    onChange(parsed.toFixed(wireDecimals))
  }

  const shown = localError ?? error

  /** Steps the stored value, not the typed text: the field may hold a half-typed number. */
  const nudge = (by: number) => {
    const current = parseNumber(text) ?? Number(value ?? 0)
    let next = current + by
    if (min !== undefined) next = Math.max(min, next)
    if (max !== undefined) next = Math.min(max, next)
    const rounded = Number(next.toFixed(wireDecimals))
    setText(displayValue(rounded.toFixed(wireDecimals), locale, fixed))
    onChange(rounded.toFixed(wireDecimals))
  }
  const atMin = min !== undefined && Number(value ?? 0) <= min
  const atMax = max !== undefined && Number(value ?? 0) >= max

  return (
    <Field label={label} error={shown} hint={hint} className={wrapperClassName}>
      {(id) => (
        <div className={cn('relative', step ? 'flex items-stretch gap-1' : undefined)}>
          {step ? (
            <Button
              size="icon"
              disabled={disabled || atMin}
              aria-label={t('action.decrease')}
              onClick={() => nudge(-step)}
            >
              <Minus className="size-4" />
            </Button>
          ) : null}
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
              setText(editValue(value, locale, fixed))
            }}
            onBlur={() => {
              setFocused(false)
              setText(displayValue(value, locale, fixed))
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
                'numeric pointer-events-none absolute inset-y-0 flex items-center',
                step ? 'right-14' : 'right-3',
                disabled ? 'text-ink-faint' : 'text-ink-soft',
              )}
            >
              {unit}
            </span>
          ) : null}
          {step ? (
            <Button
              size="icon"
              disabled={disabled || atMax}
              aria-label={t('action.increase')}
              onClick={() => nudge(step)}
            >
              <Plus className="size-4" />
            </Button>
          ) : null}
        </div>
      )}
    </Field>
  )
}

/** How many decimals to render: a money field pins them, anything else drops the zeros. */
function digits(fixed: number | null): Intl.NumberFormatOptions {
  return fixed === null
    ? { maximumFractionDigits: 3 }
    : { minimumFractionDigits: fixed, maximumFractionDigits: fixed }
}

/** Read-only rendering: grouped and locale-formatted. */
function displayValue(
  value: string | null | undefined,
  locale: 'de-DE' | 'en-US',
  fixed: number | null,
): string {
  if (value === null || value === undefined || value === '') return ''
  return formatNumber(value, locale, digits(fixed))
}

/** Editing rendering: no group separators, but the locale's decimal separator. */
function editValue(
  value: string | null | undefined,
  locale: 'de-DE' | 'en-US',
  fixed: number | null,
): string {
  if (value === null || value === undefined || value === '') return ''
  return formatNumber(value, locale, { ...digits(fixed), useGrouping: false })
}
