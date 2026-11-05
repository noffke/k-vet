import type { Drug, Packaging, ScanResult } from '@/api/generated/model'
import { freshIntake, type IntakeDraft } from './intake'

/** The pack being entered: its drug, the original packaging that is stocked, what is typed. */
interface CurrentPack {
  drug: Drug
  original: Packaging
  intake: IntakeDraft
  /** A code of this pack the drug does not know yet (its Data Matrix after its EAN, say). */
  newCode: { raw: string; display: string } | null
}

/** A code no drug has: waiting to be added to a drug, or for a new one made for it. */
interface UnknownCode {
  raw: string
  display: string
}

export interface BookedPack {
  lotId: number
  drugName: string
  batch: string | null
  expiry: string | null
  packages: number
}

/** A message for the vet — an i18n key with its values. */
interface Notice {
  key: string
  values?: Record<string, string>
}

/** A delivery being booked, pack by pack. Kept in the tab's session storage (see `persist`). */
export interface Delivery {
  current: CurrentPack | null
  unknown: UnknownCode | null
  /** Drugs whose list price was checked in this delivery: not asked again. */
  confirmed: number[]
  booked: BookedPack[]
  notice: Notice | null
}

export type DeliveryEvent =
  | { type: 'scanned'; raw: string; result: ScanResult }
  | { type: 'edited'; intake: IntakeDraft }
  | { type: 'booked'; lotId: number }
  | { type: 'discarded' }
  | { type: 'codeAdded' }
  | { type: 'undone'; lotId: number }
  | { type: 'failed'; notice: Notice }
  | { type: 'finished' }

export const emptyDelivery: Delivery = {
  current: null,
  unknown: null,
  confirmed: [],
  booked: [],
  notice: null,
}

/** What a scan carries for the lot, kept where nothing is entered yet — never over the typed. */
function fill(current: CurrentPack, result: ScanResult): CurrentPack {
  return {
    ...current,
    intake: {
      ...current.intake,
      batch: current.intake.batch || result.batch || '',
      expiry: current.intake.expiry ?? result.expiry ?? null,
    },
  }
}

/**
 * What a scan means, given the delivery so far:
 *
 * - a drug with nothing open becomes the current pack, with batch and expiry if the scan had them;
 * - the current pack's second code — a Data Matrix while batch and expiry are still empty — fills
 *   them in; any further scan of the same drug is one more pack (count up), unless its batch
 *   differs, which is another lot and waits until this one is booked;
 * - another drug while a pack is open waits too;
 * - a code no drug has waits to be added to a drug, or for a new drug.
 */
export function deliveryReducer(state: Delivery, event: DeliveryEvent): Delivery {
  const quiet = { ...state, notice: null }
  const notice = (key: string, values?: Record<string, string>): Delivery => ({
    ...state,
    notice: values ? { key, values } : { key },
  })

  switch (event.type) {
    case 'scanned': {
      const { raw, result } = event
      const carriesLot = Boolean(result.batch || result.expiry)
      const current = state.current
      if (!result.gtin && !carriesLot) return notice('code.invalid')

      if (result.drug) {
        const drug = result.drug
        if (current) {
          if (current.drug.id !== drug.id) {
            return notice('delivery.finishCurrent', { name: current.drug.name ?? '' })
          }
          if (carriesLot && !current.intake.batch && !current.intake.expiry) {
            return { ...quiet, current: fill(current, result) }
          }
          if (result.batch && current.intake.batch && result.batch !== current.intake.batch) {
            return notice('delivery.otherBatch', { batch: result.batch })
          }
          const packages = String(Number(current.intake.packages ?? '0') + 1)
          return { ...quiet, current: { ...current, intake: { ...current.intake, packages } } }
        }
        const original = result.original
        if (!original || original.draft || drug.draft) {
          return { ...notice('delivery.notStockable', { name: drug.name ?? '' }), unknown: null }
        }
        const intake = freshIntake(
          state.confirmed.includes(drug.id) ? { state: 'earlier' } : { state: 'open' },
        )
        return {
          ...quiet,
          unknown: null,
          current: {
            drug,
            original,
            intake: { ...intake, batch: result.batch ?? '', expiry: result.expiry ?? null },
            newCode: null,
          },
        }
      }

      // A code no drug has.
      if (current) {
        // Most likely this pack's other code: take what it carries, and offer to add the code.
        if (carriesLot) {
          return {
            ...quiet,
            current: {
              ...fill(current, result),
              newCode: result.gtin ? { raw, display: result.display ?? '' } : null,
            },
          }
        }
        return notice('delivery.finishCurrent', { name: current.drug.name ?? '' })
      }
      if (!result.gtin) return notice('delivery.scanProductFirst')
      return { ...quiet, unknown: { raw, display: result.display ?? '' } }
    }

    case 'edited':
      return state.current
        ? { ...quiet, current: { ...state.current, intake: event.intake } }
        : state

    case 'booked': {
      const current = state.current
      if (!current) return state
      return {
        ...quiet,
        current: null,
        confirmed: state.confirmed.includes(current.drug.id)
          ? state.confirmed
          : [...state.confirmed, current.drug.id],
        booked: [
          {
            lotId: event.lotId,
            drugName: current.drug.name ?? '',
            batch: current.intake.batch.trim() || null,
            expiry: current.intake.expiry,
            packages: Number(current.intake.packages),
          },
          ...state.booked,
        ],
      }
    }

    case 'discarded':
      return { ...quiet, current: null, unknown: null }

    case 'codeAdded':
      return state.current ? { ...quiet, current: { ...state.current, newCode: null } } : state

    case 'undone':
      return { ...quiet, booked: state.booked.filter((pack) => pack.lotId !== event.lotId) }

    case 'failed':
      return { ...state, notice: event.notice }

    case 'finished':
      return emptyDelivery
  }
}

const STORAGE_KEY = 'kvet.delivery'

/**
 * The delivery lives in the tab's session storage: it survives the detour to a new drug's page
 * and a reload, and ends with the tab or "Lieferung abschließen". What is booked is on the server
 * already; this only remembers the list and the pack in hand.
 */
export const persist = {
  load(): Delivery {
    try {
      const stored = sessionStorage.getItem(STORAGE_KEY)
      return stored
        ? { ...emptyDelivery, ...(JSON.parse(stored) as Delivery), notice: null }
        : emptyDelivery
    } catch {
      return emptyDelivery
    }
  },
  save(state: Delivery): void {
    try {
      sessionStorage.setItem(STORAGE_KEY, JSON.stringify(state))
    } catch {
      // Without storage the delivery still works; it just does not survive a reload.
    }
  },
}
