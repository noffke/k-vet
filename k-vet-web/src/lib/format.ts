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
  // No explicit digits: `style: 'currency'` already uses the currency's minor units.
  return formatNumber(value, locale, { style: 'currency', currency })
}

/**
 * The currency's own symbol ("€"), for labelling an input that holds a bare amount.
 * Taken from the same formatter as `formatMoney`, so both follow the currency in step.
 */
export function currencySymbol(locale: Locale, currency = 'EUR'): string {
  const parts = new Intl.NumberFormat(locale, { style: 'currency', currency }).formatToParts(0)
  return parts.find((part) => part.type === 'currency')?.value ?? currency
}

/**
 * The currency's minor units: 2 for the euro, 0 for the yen. A money field shows exactly
 * this many decimals, always — `14,8 €` is not a price.
 */
export function currencyDecimals(locale: Locale, currency = 'EUR'): number {
  const resolved = new Intl.NumberFormat(locale, { style: 'currency', currency }).resolvedOptions()
  // Every currency resolves to a digit count; the type allows for formatters that do not.
  return resolved.maximumFractionDigits ?? 2
}

/**
 * Percentages as stored (e.g. "19.000" → "19 %"). The separator is a non-breaking space,
 * the same one Intl puts before the currency sign, so a narrow column wraps the whole
 * value instead of stranding the "%" on its own line. `parseNumber` strips it again.
 */
export function formatPercent(value: string | number | null | undefined, locale: Locale): string {
  const parsed = toNumber(value)
  if (parsed === null) return ''
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 3 }).format(parsed)}\u00a0%`
}

/**
 * Rewrites the other locale's decimal separator as this one's.
 *
 * A numeric keypad emits a dot wherever it is sold, so a German vet typing `10.3` means ten
 * point three, not one hundred and three — and every field here is a price, a weight or a
 * quantity, none of which are typed with a thousands separator. A separator that *can* only be
 * a group separator is left alone: one that sits beside an explicit decimal comma
 * (`1.234,56`), or several of them (`1.234.567`).
 */
export function localiseSeparators(input: string, locale: Locale): string {
  const { group, decimal } = separators(locale)
  if (input.includes(decimal)) return input
  const parts = input.split(group)
  return parts.length === 2 ? parts.join(decimal) : input
}

/**
 * Parses user input in the active locale. Accepts the locale's decimal separator, reads the
 * other locale's as a decimal separator where it cannot be a group separator, and ignores
 * group separators and spaces. Returns `null` for anything that is not a number.
 */
export function parseNumber(input: string, locale: Locale): number | null {
  const trimmed = input.trim()
  if (trimmed === '') return null
  const { group, decimal } = separators(locale)
  let normalized = localiseSeparators(trimmed, locale)
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
 * `HH:mm` → the locale's time of day: "15:00" in de-DE, "03:00 PM" in en-US.
 *
 * Unlike `formatTime` this takes a time on its own, with no date and no timezone to shift
 * it — what a time field holds while it is being edited.
 */
export function formatTimeOfDay(value: string | null | undefined, locale: Locale): string {
  if (!value) return ''
  const [hours, minutes] = value.split(':').map(Number)
  if (!Number.isInteger(hours) || !Number.isInteger(minutes)) return ''
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
  }).format(new Date(Date.UTC(2000, 0, 1, hours, minutes)))
}

/**
 * Parses a typed time into `HH:mm`, the 24-hour form the rest of the app stores.
 *
 * Deliberately forgiving, because this is typed between two other fields: `15:00`, `15.00`,
 * `1500` and `15` all mean three in the afternoon, and `3pm` / `03:00 PM` are accepted too
 * so that the en-US habit works without switching the whole app.
 */
export function parseTime(input: string): string | null {
  const trimmed = input.trim().toLowerCase()
  if (trimmed === '') return null

  const suffix = /(a|p)\.?m\.?$/.exec(trimmed)
  const digits = trimmed.replace(/(a|p)\.?m\.?$/, '').trim()

  // `1500` and `930` are the keypad forms; everything else separates hour from minute.
  const compact = /^\d{3,4}$/.exec(digits)
  const parts = compact
    ? [digits.slice(0, digits.length - 2), digits.slice(-2)]
    : digits.split(/[:.\s]+/).filter((part) => part !== '')
  if (parts.length > 2) return null

  const [rawHours = '', rawMinutes = '0'] = parts
  if (!/^\d{1,2}$/.test(rawHours) || !/^\d{1,2}$/.test(rawMinutes)) return null
  let hours = Number(rawHours)
  const minutes = Number(rawMinutes)

  if (suffix) {
    if (hours < 1 || hours > 12) return null
    if (suffix[1] === 'p') hours = hours === 12 ? 12 : hours + 12
    else if (hours === 12) hours = 0
  }
  if (hours > 23 || minutes > 59) return null

  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(hours)}:${pad(minutes)}`
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
  return isoDate(expandYear(third), month, day)
}

/**
 * Two typed digits become a year. Appointments are in the near future and dates of birth
 * in the past, so the split is the usual one: 70 and above reads as the last century.
 */
function expandYear(year: number): number {
  if (year >= 100) return year
  return year >= 70 ? 1900 + year : 2000 + year
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
