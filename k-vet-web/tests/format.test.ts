import { describe, expect, it } from 'vitest'
import {
  formatDate,
  formatMoney,
  formatPercent,
  formatQuantity,
  parseDate,
  parseNumber,
  parseNumberToWire,
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
