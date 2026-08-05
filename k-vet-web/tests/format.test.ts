import { describe, expect, it } from 'vitest'
import {
  currencySymbol,
  formatDate,
  formatMoney,
  formatPercent,
  formatQuantity,
  formatTimeOfDay,
  parseDate,
  parseNumber,
  parseNumberToWire,
  parseTime,
  toWire,
} from '@/lib/format'

describe('formatting in de-DE', () => {
  it('formats money with decimal comma and trailing currency', () => {
    // Intl separates amount and currency with a non-breaking space.
    expect(formatMoney('1234.5', 'de-DE')).toBe('1.234,50 €')
  })

  it('drops trailing zeros on quantities', () => {
    expect(formatQuantity('1.50', 'de-DE')).toBe('1,5')
    expect(formatQuantity('100.00', 'de-DE')).toBe('100')
  })

  it('formats percentages as stored, kept on one line', () => {
    // A non-breaking space, so a narrow table column never wraps the "%" onto its own line.
    expect(formatPercent('19.000', 'de-DE')).toBe('19\u00a0%')
    expect(formatPercent('19.000', 'de-DE')).not.toContain(' ')
  })

  it('gives the currency sign on its own, for fields holding a bare amount', () => {
    expect(currencySymbol('de-DE')).toBe('€')
    expect(currencySymbol('de-DE', 'CHF')).toBe('CHF')
  })

  it('formats dates as dd.MM.yyyy without shifting the day', () => {
    expect(formatDate('2026-05-04', 'de-DE')).toBe('04.05.2026')
    expect(formatDate('2026-01-01T23:30:00Z', 'de-DE')).toBe('01.01.2026')
  })
})

describe('formatting in en-US', () => {
  it('formats money with decimal point and leading currency', () => {
    expect(formatMoney('1234.5', 'en-US')).toBe('€1,234.50')
  })

  it('formats dates as M/d/yyyy', () => {
    expect(formatDate('2026-05-04', 'en-US')).toBe('05/04/2026')
  })
})

describe('times of day', () => {
  it('shows a time in the locale it is read in', () => {
    expect(formatTimeOfDay('15:00', 'de-DE')).toBe('15:00')
    expect(formatTimeOfDay('15:00', 'en-US')).toBe('03:00 PM')
    expect(formatTimeOfDay('', 'de-DE')).toBe('')
  })

  it('takes every shape the time is typed in', () => {
    // The native control this replaced accepted only its own browser-locale format.
    expect(parseTime('15:00')).toBe('15:00')
    expect(parseTime('15.00')).toBe('15:00')
    expect(parseTime('1500')).toBe('15:00')
    expect(parseTime('15')).toBe('15:00')
    expect(parseTime('9:5')).toBe('09:05')
    expect(parseTime('930')).toBe('09:30')
  })

  it('understands am and pm, so an en-US habit still types cleanly', () => {
    expect(parseTime('3pm')).toBe('15:00')
    expect(parseTime('03:00 PM')).toBe('15:00')
    expect(parseTime('12:30 am')).toBe('00:30')
    expect(parseTime('12:30 pm')).toBe('12:30')
  })

  it('refuses an impossible time', () => {
    expect(parseTime('25:00')).toBeNull()
    expect(parseTime('12:61')).toBeNull()
    expect(parseTime('13pm')).toBeNull()
    expect(parseTime('Mittag')).toBeNull()
    expect(parseTime('')).toBeNull()
  })
})

describe('parsing numbers', () => {
  it('accepts the German decimal comma and group separator', () => {
    expect(parseNumber('1,5', 'de-DE')).toBe(1.5)
    expect(parseNumber('1.234,56', 'de-DE')).toBe(1234.56)
    expect(parseNumber('-2,5', 'de-DE')).toBe(-2.5)
  })

  it('accepts the US decimal point and group separator', () => {
    expect(parseNumber('1.5', 'en-US')).toBe(1.5)
    expect(parseNumber('1,234.56', 'en-US')).toBe(1234.56)
  })

  it('rejects text that is not a number', () => {
    expect(parseNumber('abc', 'de-DE')).toBeNull()
    expect(parseNumber('1,2,3', 'de-DE')).toBeNull()
    expect(parseNumber('', 'de-DE')).toBeNull()
    expect(parseNumber('-', 'de-DE')).toBeNull()
  })

  it('treats a US decimal point in de-DE as a group separator', () => {
    // "1.500" is one thousand five hundred to a German user.
    expect(parseNumber('1.500', 'de-DE')).toBe(1500)
  })

  it('produces dot-decimal wire values from either locale', () => {
    expect(parseNumberToWire('1,5', 'de-DE')).toBe('1.50')
    expect(parseNumberToWire('1.5', 'en-US')).toBe('1.50')
    expect(parseNumberToWire('100', 'de-DE', 3)).toBe('100.000')
    expect(toWire(12.345, 2)).toBe('12.35')
  })
})

describe('parsing dates', () => {
  it('reads dd.MM.yyyy in de-DE', () => {
    expect(parseDate('04.05.2026', 'de-DE')).toBe('2026-05-04')
    expect(parseDate('4.5.2026', 'de-DE')).toBe('2026-05-04')
  })

  it('reads M/d/yyyy in en-US', () => {
    expect(parseDate('5/4/2026', 'en-US')).toBe('2026-05-04')
  })

  it('expands a short date, two-digit year and all', () => {
    // What the vet actually types on a phone: no padding, no century.
    expect(parseDate('5.8.26', 'de-DE')).toBe('2026-08-05')
    expect(parseDate('5/8/26', 'en-US')).toBe('2026-05-08')
  })

  it('reads a two-digit year from 70 as the last century', () => {
    // The usual split, so a date of birth in the 1900s is reachable at all.
    expect(parseDate('4.5.69', 'de-DE')).toBe('2069-05-04')
    expect(parseDate('4.5.70', 'de-DE')).toBe('1970-05-04')
    expect(parseDate('4.5.98', 'de-DE')).toBe('1998-05-04')
    // A written-out year is taken as it stands.
    expect(parseDate('4.5.1998', 'de-DE')).toBe('1998-05-04')
  })

  it('accepts ISO input in both locales', () => {
    expect(parseDate('2026-05-04', 'de-DE')).toBe('2026-05-04')
    expect(parseDate('2026-05-04', 'en-US')).toBe('2026-05-04')
  })

  it('rejects impossible dates', () => {
    expect(parseDate('31.02.2026', 'de-DE')).toBeNull()
    expect(parseDate('13/40/2026', 'en-US')).toBeNull()
    expect(parseDate('nonsense', 'de-DE')).toBeNull()
  })
})
