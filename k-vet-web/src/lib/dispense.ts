import type { TFunction } from 'i18next'
import type { Dispense, DispenseRule } from '@/api/generated/model'

/**
 * A drug position is what was dispensed, in the original pack's unit (migration 0039): whole
 * packs are billed at the pack price, the rest as a Teilmenge. These phrase a drug's rule and a
 * line's split for the pages that edit a drug quantity — a treatment line, a Behandlungsgruppe,
 * a Preisauskunft, a Mitbringen reminder. Field errors from the server carry a key, not values,
 * so the values come from the record's own rule.
 */

type Format = (value: string | number | null | undefined) => string

/** `10 ml`, `5 Tbl.` */
function amount(value: string, unit: string | null | undefined, quantity: Format): string {
  return `${quantity(value)} ${unit ?? ''}`.trim()
}

/** How a drug can be dispensed: "in Schritten von 10 ml", "beliebige Menge", "nur ganze Packungen zu 100 ml". */
export function dispenseRuleText(
  rule: DispenseRule,
  unit: string | null | undefined,
  t: TFunction,
  quantity: Format,
): string {
  if (rule.freely_divisible) return t('dispense.anyQuantity')
  if (rule.step) return t('dispense.inSteps', { step: amount(rule.step, unit, quantity) })
  return t('dispense.wholePacks', { size: amount(rule.pack_size, unit, quantity) })
}

/** The quantity refusals that are about the drug's steps, which its rule can spell out. */
const STEP_REFUSALS = new Set(['item.subsetStep', 'item.wholePacksOnly'])

/**
 * What a refused write says: for a quantity off the drug's steps, the steps themselves
 * ("Diese Menge geht nicht: in Schritten von 10 ml."); otherwise the key's own text.
 */
export function refusalText(
  key: string,
  rule: DispenseRule | null | undefined,
  unit: string | null | undefined,
  t: TFunction,
  quantity: Format,
): string {
  if (rule && STEP_REFUSALS.has(key)) {
    return t('dispense.refused', { rule: dispenseRuleText(rule, unit, t, quantity) })
  }
  return t(key)
}

/** A line's split as the invoice prints it: "1 × 100 ml à 14,88 € + 50 ml Teilmenge à 0,298 €/ml". */
export function dispenseBreakdown(
  dispense: Dispense,
  unit: string | null | undefined,
  t: TFunction,
  format: { quantity: Format; money: Format; unitPrice: Format },
): string {
  const parts: string[] = []
  if (dispense.packs) {
    parts.push(
      t('dispense.packsPart', {
        count: format.quantity(dispense.packs.quantity),
        size: amount(dispense.rule.pack_size, unit, format.quantity),
        price: format.money(dispense.packs.price_gross),
      }),
    )
  }
  if (dispense.remainder) {
    parts.push(
      t('dispense.restPart', {
        amount: amount(dispense.remainder.quantity, unit, format.quantity),
        price: format.unitPrice(dispense.remainder.price_gross),
        unit: unit ?? '',
      }),
    )
  }
  return parts.join(' + ')
}
