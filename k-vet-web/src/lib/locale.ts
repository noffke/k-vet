import { useTranslation } from 'react-i18next'
import {
  currencySymbol,
  formatDate,
  formatDateTime,
  formatMoney,
  formatNumber,
  formatPercent,
  formatQuantity,
  formatTime,
  type Locale,
  parseDate,
  parseNumber,
  parseNumberToWire,
} from '@/lib/format'
import { currentLocale } from '@/lib/i18n'

/**
 * Locale-bound formatters and parsers for components.
 *
 * Every screen formats through this hook, so switching the language re-renders
 * numbers, dates and money in the new locale's conventions (SC-010).
 */
export function useLocaleFormat() {
  // Subscribing to i18next re-renders the caller on a language switch.
  const { i18n } = useTranslation()
  const locale: Locale = (['de-DE', 'en-US'] as string[]).includes(i18n.language)
    ? (i18n.language as Locale)
    : currentLocale()

  return {
    locale,
    /** The currency sign alone — for a field that holds the amount without it. */
    currencySymbol: currencySymbol(locale),
    number: (value: string | number | null | undefined, options?: Intl.NumberFormatOptions) =>
      formatNumber(value, locale, options),
    quantity: (value: string | number | null | undefined) => formatQuantity(value, locale),
    money: (value: string | number | null | undefined) => formatMoney(value, locale),
    percent: (value: string | number | null | undefined) => formatPercent(value, locale),
    date: (value: string | null | undefined) => formatDate(value, locale),
    dateTime: (value: string | null | undefined) => formatDateTime(value, locale),
    time: (value: string | null | undefined) => formatTime(value, locale),
    parseNumber: (input: string) => parseNumber(input, locale),
    parseNumberToWire: (input: string, decimals?: number) =>
      parseNumberToWire(input, locale, decimals),
    parseDate: (input: string) => parseDate(input, locale),
  }
}
