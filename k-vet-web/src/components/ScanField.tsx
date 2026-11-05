import { type KeyboardEvent, type Ref, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { TextField } from '@/components/ui/field'

/** Ends a variable-length field inside a Data Matrix (FNC1 / GS). */
const GS = '\u001d'

/** Editing keys a person may still press in the field; every other Ctrl combination is a
 *  scanner's control character and must not reach the browser as a shortcut. */
const EDITING = new Set(['a', 'c', 'v', 'x', 'y', 'z'])

interface ScanFieldProps {
  label: string
  /** Called with exactly what was typed or scanned, once it ends with Enter. */
  onScan: (raw: string) => void
  /** Enter on the empty field — the delivery page books the pack in hand with it. */
  onEnterEmpty?: (() => void) | undefined
  placeholder?: string | undefined
  hint?: string | undefined
  error?: string | undefined
  autoFocus?: boolean | undefined
  disabled?: boolean | undefined
  wrapperClassName?: string | undefined
  ref?: Ref<HTMLInputElement> | undefined
}

/**
 * Where a barcode scanner types. A scanner set up as a USB keyboard types what it reads and
 * presses Enter; this field hands that over whole and empties itself for the next pack. A
 * person can type a code just the same.
 *
 * Inside a Data Matrix, the group separator ends a variable field (the batch). A keyboard
 * scanner sends it as Ctrl+] — which a German layout turns into Ctrl and the key "+" sits on —
 * so both come back here as the separator instead of zooming the page. Other Ctrl combinations
 * a scanner may send (Ctrl+D would bookmark the page) are swallowed.
 *
 * Under the field, "Zuletzt gelesen" shows what arrived, control characters made visible: what
 * to look at when setting up a scanner.
 */
export function ScanField({
  label,
  onScan,
  onEnterEmpty,
  placeholder,
  hint,
  error,
  autoFocus,
  disabled,
  wrapperClassName,
  ref,
}: ScanFieldProps) {
  const { t } = useTranslation()
  const [value, setValue] = useState('')
  const [last, setLast] = useState<string | null>(null)

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      if (!value.trim()) {
        onEnterEmpty?.()
        return
      }
      setValue('')
      setLast(value)
      onScan(value)
      return
    }
    // AltGr is Ctrl+Alt on Windows; those keys type characters (@, |, …) and stay alone.
    if ((event.ctrlKey || event.metaKey) && !event.altKey) {
      const key = event.key.toLowerCase()
      if (key === ']' || key === '+') {
        event.preventDefault()
        setValue((current) => current + GS)
      } else if (!EDITING.has(key)) {
        event.preventDefault()
      }
    }
  }

  const readout = last === null ? null : `${t('scan.lastRead')}: ${visible(last)}`
  return (
    <TextField
      ref={ref}
      label={label}
      value={value}
      placeholder={placeholder}
      autoFocus={autoFocus}
      disabled={disabled}
      autoComplete="off"
      spellCheck={false}
      wrapperClassName={wrapperClassName}
      error={error}
      hint={readout ?? hint}
      onChange={(event) => setValue(event.target.value)}
      onKeyDown={onKeyDown}
    />
  )
}

/** Control characters as the symbols for them, so a scan can be read off the screen. */
function visible(raw: string): string {
  return Array.from(raw, (character) => {
    const code = character.charCodeAt(0)
    return code < 0x20 ? String.fromCharCode(0x2400 + code) : character
  }).join('')
}
