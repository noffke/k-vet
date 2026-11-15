import i18n from 'i18next'
import { beforeAll, describe, expect, it } from 'vitest'
import type { Dispense, DispenseRule } from '@/api/generated/model'
import de from '@/i18n/de.json'
import { dispenseBreakdown, dispenseRuleText, quantityStep, refusalText } from '@/lib/dispense'
import { formatMoney, formatQuantity, formatUnitPrice } from '@/lib/format'

const quantity = (value: string | number | null | undefined) => formatQuantity(value, 'de-DE')
const money = (value: string | number | null | undefined) => formatMoney(value, 'de-DE')
const unitPrice = (value: string | number | null | undefined) => formatUnitPrice(value, 'de-DE')

/** The seeded bottle: 100 ml, in steps of its 10 ml Teilmenge. */
const steps: DispenseRule = {
  pack_size: '100.00',
  step: '10.00',
  freely_divisible: false,
  preset: '10.00',
}

beforeAll(async () => {
  await i18n.init({ lng: 'de', resources: { de: { translation: de } } })
})

/**
 * How a drug quantity field explains itself (migration 0039): the drug's steps beside the field,
 * the same steps in a refusal, and the split the invoice prints.
 */
describe('dispensing a drug', () => {
  const t = i18n.t.bind(i18n)

  it('says how a drug can be dispensed', () => {
    expect(dispenseRuleText(steps, 'ml', t, quantity)).toBe('in Schritten von 10 ml')
    expect(dispenseRuleText({ ...steps, step: null }, 'ml', t, quantity)).toBe(
      'nur ganze Packungen zu 100 ml',
    )
    expect(
      dispenseRuleText({ ...steps, step: null, freely_divisible: true }, 'ml', t, quantity),
    ).toBe('beliebige Menge')
  })

  it('spells out the steps when a quantity is refused for them, and only then', () => {
    expect(refusalText('item.subsetStep', steps, 'ml', t, quantity)).toBe(
      'Diese Menge geht nicht: in Schritten von 10 ml.',
    )
    expect(refusalText('item.stockShort', steps, 'ml', t, quantity)).toBe(de.item.stockShort)
    // Without a rule to name, the key's own text.
    expect(refusalText('item.subsetStep', null, 'ml', t, quantity)).toBe(de.item.subsetStep)
  })

  it('reads a line as the invoice prints it', () => {
    const dispense: Dispense = {
      rule: steps,
      packs: { quantity: '1', price_gross: '14.88', line_net: '12.50', line_gross: '14.88' },
      remainder: {
        quantity: '50.00',
        price_gross: '0.298',
        line_net: '12.50',
        line_gross: '14.88',
      },
    }
    expect(dispenseBreakdown(dispense, 'ml', t, { quantity, money, unitPrice })).toBe(
      '1 × 100 ml à 14,88 € + 50 ml Teilmenge à 0,298 €/ml',
    )
    expect(
      dispenseBreakdown({ ...dispense, packs: null }, 'ml', t, { quantity, money, unitPrice }),
    ).toBe('50 ml Teilmenge à 0,298 €/ml')
  })

  it('steps a quantity field by the Teilmenge, one unit, or a whole pack', () => {
    expect(quantityStep(steps)).toBe(10)
    expect(quantityStep({ ...steps, step: null })).toBe(100)
    expect(quantityStep({ ...steps, step: null, freely_divisible: true })).toBe(1)
  })
})

describe('a unit price', () => {
  it('shows up to four decimals and never fewer than the currency has', () => {
    expect(unitPrice('0.238')).toBe('0,238 €')
    expect(unitPrice('0.30')).toBe('0,30 €')
    expect(unitPrice('2.8251')).toBe('2,8251 €')
    expect(formatUnitPrice('0.238', 'en-US')).toBe('€0.238')
  })
})
