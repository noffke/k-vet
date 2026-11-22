import { PencilLine } from 'lucide-react'
import type { KeyboardEvent, ReactNode } from 'react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NumberInput } from '@/components/NumberInput'
import { closeWhenReleased } from '@/lib/pointer'
import { cn } from '@/lib/utils'

interface ValueCellProps {
  /** What the value is — "Menge", "Faktor", "Kilometer": the field's label, said not shown. */
  label: string
  /** The value as it reads in the row: "2 ×", "150 ml", "230 %", "km fehlt". */
  display: string
  /** Wire value for the editor. */
  value: string | null | undefined
  unit?: string | undefined
  decimals?: number | undefined
  /** Whether the editor is shown; the parent decides (the newest line keeps it open). */
  open: boolean
  onOpen: () => void
  /** Asked when the editor is left — by Enter, Escape or the focus moving away. */
  onClose: () => void
  /** Focus the editor, value selected, as soon as it is open. */
  autoFocus?: boolean | undefined
  onChange: (value: string | null) => void
  /** Enter pressed in the editor: where the focus goes next (the search, after a pick). */
  onEnter?: (() => void) | undefined
  /** A value still missing, said in the danger colour ("km fehlt"). */
  missing?: boolean | undefined
  /** A button inside the open editor — the kilometres from Google Maps. */
  action?: { label: string; icon: ReactNode; onClick: () => void; disabled?: boolean } | undefined
  className?: string | undefined
}

/**
 * One editable number in a position's row: the value as text until it is wanted, then the
 * field to type it in. The whole cell is the button — a tap anywhere on the number, its unit or
 * the pencil opens it — and the field is typed into, never stepped: quantities are rarely 1–3
 * (decided 2026-11-21).
 */
export function ValueCell({
  label,
  display,
  value,
  unit,
  decimals,
  open,
  onOpen,
  onClose,
  autoFocus,
  onChange,
  onEnter,
  missing,
  action,
  className,
}: ValueCellProps) {
  const { t } = useTranslation()
  const input = useRef<HTMLInputElement | null>(null)
  // Opened by a tap: the field takes the focus at once, as the tap meant.
  const [focusWhenOpen, setFocusWhenOpen] = useState(false)

  useEffect(() => {
    if (!open || !(autoFocus || focusWhenOpen)) return
    const field = input.current
    if (!field) return
    // The newest line takes the focus from the empty search it was picked from — not from a
    // field the vet is already typing in again (the next search, before the line came in).
    if (!focusWhenOpen && typingElsewhere(field)) return
    field.focus()
    // The field shows its edit form on focus; select that, so typing replaces it.
    requestAnimationFrame(() => field.select())
    setFocusWhenOpen(false)
  }, [open, autoFocus, focusWhenOpen])

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => {
          setFocusWhenOpen(true)
          onOpen()
        }}
        className={cn(
          'group numeric flex min-h-11 w-full items-center justify-end gap-1.5 whitespace-nowrap rounded-control px-2 text-sm sm:min-h-9',
          'transition-colors hover:bg-sunken',
          missing ? 'font-semibold text-danger' : 'text-ink',
          className,
        )}
      >
        {/* Named by its content, not `aria-label`: a field's label ("Menge") must find the
            field, not the button that opens it. */}
        <span className="sr-only">{t('position.edit', { label })}</span>
        {display}
        <PencilLine
          className="size-3.5 shrink-0 text-ink-faint group-hover:text-rust"
          aria-hidden
        />
      </button>
    )
  }

  const keyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      if (onEnter) onEnter()
      else event.currentTarget.blur()
    } else if (event.key === 'Escape') {
      event.currentTarget.blur()
    }
  }

  return (
    // Closed when the focus leaves the cell, not the input: the button inside it (the
    // kilometres from Google Maps) must still take its click.
    <fieldset
      // As wide as a few digits and its unit need — `NumberInput` keeps that much for them —
      // and never narrower than the usual cell (more with a button); not the input's default
      // 20 characters, which widened the whole column while it was open. A fixed width let a
      // long unit ("Originalpackung") run out of it over the amount beside it.
      className={cn('m-0 w-min border-0 p-0', action ? 'min-w-48' : 'min-w-36', className)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          closeWhenReleased(onClose)
        }
      }}
    >
      <NumberInput
        label={label}
        hideLabel
        value={value}
        unit={unit}
        decimals={decimals}
        inputRef={(node) => {
          input.current = node
        }}
        onKeyDown={keyDown}
        onChange={onChange}
        action={action}
      />
    </fieldset>
  )
}

/** Whether the focus is in another field that already holds something typed. */
function typingElsewhere(field: HTMLInputElement): boolean {
  const active = document.activeElement
  if (!active || active === field) return false
  if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
    return active.value !== ''
  }
  return active instanceof HTMLElement && active.isContentEditable
}
