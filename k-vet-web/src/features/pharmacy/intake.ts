import type { CreateIntake, Packaging } from '@/api/generated/model'
import { todayIso } from '@/lib/format'

/**
 * Whether the vet has checked the list price for this delivery. Wholesaler prices change, and a
 * changed one is easily missed — so a Wareneingang asks: still right (`same`), or a new one
 * (`changed`, with the price typed). `earlier` is `same` said for this drug earlier in the same
 * delivery, which is not asked again.
 */
export type PriceCheck =
  | { state: 'open' }
  | { state: 'same' }
  | { state: 'earlier' }
  | { state: 'changed'; value: string | null }

/** One Wareneingang as it is being entered: what arrived, and the price check. */
export interface IntakeDraft {
  arrivalDate: string | null
  packages: string | null
  batch: string
  expiry: string | null
  price: PriceCheck
}

export function freshIntake(price: PriceCheck = { state: 'open' }): IntakeDraft {
  return { arrivalDate: todayIso(), packages: '1', batch: '', expiry: null, price }
}

/** Ready to book: at least one package, and the price checked. */
export function canBook(draft: IntakeDraft): boolean {
  const packages = Number(draft.packages)
  const priced =
    draft.price.state === 'same' ||
    draft.price.state === 'earlier' ||
    (draft.price.state === 'changed' && Number(draft.price.value) > 0)
  return Number.isInteger(packages) && packages > 0 && priced
}

/** The request: a changed price goes with it and is stored together with the lot. */
export function intakeBody(draft: IntakeDraft, original: Packaging): CreateIntake {
  return {
    arrival_date: draft.arrivalDate,
    packages_received: Number(draft.packages),
    batch_number: draft.batch.trim() || null,
    expiration_date: draft.expiry,
    list_price_net:
      draft.price.state === 'changed' ? draft.price.value : (original.list_price_net ?? null),
  }
}
