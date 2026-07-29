/**
 * Locale-aware formatting and parsing (Constitution III, research R3).
 *
 * The wire format is locale-independent: ISO 8601 dates and dot-decimal strings for
 * every NUMERIC column. Everything a person reads or types goes through this module —
 * German decimal comma and dd.MM.yyyy, US decimal point and M/d/yyyy — so no screen
 * ever falls back to `toString()` on a number, a date or an amount.
 */

export type Locale = 'de-DE' | 'en-US'

export const DEFAULT_LOCALE: Locale = 'de-DE'

/** Number of decimals used for money and quantities on the wire. */
const WIRE_DECIMALS = 2

interface Separators {
  group: string
  decimal: string
}

const separatorCache = new Map<string, Separators>()

/** The group and decimal separators the active locale uses. */
function separators(locale: Locale): Separators {
  const cached = separatorCache.get(locale)
  if (cached) return cached
  const parts = new Intl.NumberFormat(locale).formatToParts(12345.6)
  const found: Separators = {
    group: parts.find((part) => part.type === 'group')?.value ?? ',',
    decimal: parts.find((part) => part.type === 'decimal')?.value ?? '.',
  }
  separatorCache.set(locale, found)
  return found
}

/** Turns a wire value (dot-decimal string, number, null) into a number. */
function toNumber(value: string | number | null | undefined): number | null {
  if (value === null || value === undefined || value === '') return null
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

export function formatNumber(
  value: string | number | null | undefined,
  locale: Locale,
  options: Intl.NumberFormatOptions = {},
): string {
  const parsed = toNumber(value)
  if (parsed === null) return ''
  return new Intl.NumberFormat(locale, options).format(parsed)
}

/** Quantities: up to two decimals, trailing zeros dropped ("1,5" not "1,50"). */
export function formatQuantity(value: string | number | null | undefined, locale: Locale): string {
  return formatNumber(value, locale, { maximumFractionDigits: 2 })
}

/** Money with the practice currency: "1.234,56 €" in de-DE, "€1,234.56" in en-US. */
export function formatMoney(
  value: string | number | null | undefined,
  locale: Locale,
  currency = 'EUR',
): string {
  return formatNumber(value, locale, {
    style: 'currency',
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

/** Percentages as stored (e.g. "19.000" → "19 %"). */
export function formatPercent(value: string | number | null | undefined, locale: Locale): string {
  const parsed = toNumber(value)
  if (parsed === null) return ''
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 3 }).format(parsed)} %`
}

/**
 * Parses user input in the active locale. Accepts the locale's decimal separator and
 * ignores group separators and spaces. Returns `null` for anything that is not a number.
 */
export function parseNumber(input: string, locale: Locale): number | null {
  const trimmed = input.trim()
  if (trimmed === '') return null
  const { group, decimal } = separators(locale)
  let normalized = trimmed
    .replaceAll(group, '')
    .replaceAll(' ', '')
    .replaceAll(' ', '')
    .replaceAll(' ', '')
  if (decimal !== '.') normalized = normalized.replaceAll(decimal, '.')
  // A stray separator of the *other* locale is a typo, not a group separator.
  if (!/^[+-]?\d*\.?\d*$/.test(normalized)) return null
  if (normalized === '' || normalized === '.' || normalized === '-' || normalized === '+') {
    return null
  }
  const parsed = Number(normalized)
  return Number.isFinite(parsed) ? parsed : null
}

/** Formats a number for the wire: fixed decimals, always dot-separated. */
export function toWire(value: number, decimals = WIRE_DECIMALS): string {
  return value.toFixed(decimals)
}

/** Parses user input straight into a wire string, or `null` when invalid. */
export function parseNumberToWire(
  input: string,
  locale: Locale,
  decimals = WIRE_DECIMALS,
): string | null {
  const parsed = parseNumber(input, locale)
  return parsed === null ? null : toWire(parsed, decimals)
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/** ISO date (`yyyy-MM-dd`) → locale short date. Never shifts by a timezone. */
export function formatDate(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return ''
  const datePart = iso.slice(0, 10)
  const [year, month, day] = datePart.split('-').map(Number)
  if (!year || !month || !day) return ''
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: 'UTC',
  }).format(new Date(Date.UTC(year, month - 1, day)))
}

/** ISO timestamp → locale date and time in the browser's timezone. */
export function formatDateTime(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return ''
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}

/** ISO timestamp → `HH:mm` in the browser's timezone. */
export function formatTime(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return ''
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  return new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(date)
}

/**
 * Parses a typed date in the active locale into an ISO date.
 * Accepts `dd.MM.yyyy` (de-DE), `M/d/yyyy` (en-US) and ISO input in both.
 */
export function parseDate(input: string, locale: Locale): string | null {
  const trimmed = input.trim()
  if (trimmed === '') return null
  const iso = /^(\d{4})-(\d{2})-(\d{2})$/.exec(trimmed)
  if (iso) return isoDate(Number(iso[1]), Number(iso[2]), Number(iso[3]))

  const parts = trimmed.split(/[./-]/).filter((part) => part !== '')
  if (parts.length !== 3) return null
  const numbers = parts.map(Number)
  if (numbers.some((value) => !Number.isInteger(value))) return null

  const [first = 0, second = 0, third = 0] = numbers
  const { day, month } =
    locale === 'en-US' ? { month: first, day: second } : { day: first, month: second }
  const year = third < 100 ? 2000 + third : third
  return isoDate(year, month, day)
}

function isoDate(year: number, month: number, day: number): string | null {
  if (month < 1 || month > 12 || day < 1 || day > 31) return null
  const date = new Date(Date.UTC(year, month - 1, day))
  if (date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return null
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${year}-${pad(month)}-${pad(day)}`
}

/** Today as an ISO date in the browser's timezone. */
export function todayIso(): string {
  const now = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
}
