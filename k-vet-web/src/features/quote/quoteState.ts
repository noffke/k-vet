import { useEffect, useState } from 'react'
import type { TreatmentItemKind } from '@/api/generated/model'

/**
 * The Preisauskunft lives in the browser tab and nowhere else: nothing about it is saved on
 * the server. `sessionStorage` keeps it across a reload, as `lib/trail.ts` keeps the trail.
 * It holds what the vet chose — positions, quantities, factors, kilometres — never a price:
 * those come from the server, which prices a quote as it prices a treatment.
 */

/** One position of the quote, as the vet set it. */
export interface DraftLine {
  /** Stable while the line exists, for React and for editing one line; not a database id. */
  key: string
  kind: TreatmentItemKind
  /** The drug packaging's or the service's id. */
  id: number
  /** Shown until the server's answer names the line. */
  name: string
  /** Wire values (dot-decimal strings), as the API takes them. */
  quantity: string
  /** Services only; `null` prices at the catalogue's factor. */
  factor: string | null
  km: string | null
  kmMultiplier: string | null
  /** The vet typed the kilometres herself: a lookup never overwrites them. */
  kmTyped: boolean
  /** The destination the kilometres were last looked up for, so each is asked once. */
  kmFor: string | null
}

export interface QuoteAddress {
  street: string
  zip: string
  city: string
}

export interface QuoteState {
  /** An existing customer — or none, and perhaps a typed address instead. Their name and home
   * are read from the server, which keeps them current. */
  customer: { id: number } | null
  address: QuoteAddress
  /** The vet has finished typing the address and moved on: only then is it asked about. */
  addressDone: boolean
  lines: DraftLine[]
}

export const EMPTY_QUOTE: QuoteState = {
  customer: null,
  address: { street: '', zip: '', city: '' },
  addressDone: false,
  lines: [],
}

const STORAGE_KEY = 'kvet.quote'

let counter = 0
/** `crypto.randomUUID` needs a secure context, which a practice LAN on plain HTTP is not. */
function newKey(): string {
  counter += 1
  return `${Date.now().toString(36)}-${counter}`
}

/** What a picked position or a group's entry brings along. */
export interface PickedPosition {
  kind: TreatmentItemKind
  id: number
  name: string
  quantity?: string
}

export function addPositions(state: QuoteState, picked: PickedPosition[]): QuoteState {
  const lines = picked.map(
    (position): DraftLine => ({
      key: newKey(),
      kind: position.kind,
      id: position.id,
      name: position.name,
      quantity: position.quantity ?? '1.00',
      factor: null,
      km: null,
      kmMultiplier: null,
      kmTyped: false,
      kmFor: null,
    }),
  )
  return { ...state, lines: [...state.lines, ...lines] }
}

export function updateLine(state: QuoteState, key: string, patch: Partial<DraftLine>): QuoteState {
  return {
    ...state,
    lines: state.lines.map((line) => (line.key === key ? { ...line, ...patch } : line)),
  }
}

export function removeLine(state: QuoteState, key: string): QuoteState {
  return { ...state, lines: state.lines.filter((line) => line.key !== key) }
}

/** A customer replaces a typed address: the drive goes to their home. */
export function setCustomer(state: QuoteState, customer: QuoteState['customer']): QuoteState {
  return { ...state, customer, address: EMPTY_QUOTE.address, addressDone: false }
}

/** Typing an address is for a caller who is not a customer yet. */
export function setAddress(state: QuoteState, address: QuoteAddress): QuoteState {
  return { ...state, customer: null, address, addressDone: false }
}

/** The address is complete enough to drive to. */
function addressComplete(address: QuoteAddress): boolean {
  return [address.street, address.zip, address.city].every((part) => part.trim() !== '')
}

/**
 * Where the Wegegeld goes, as one key: the customer, or a typed address once the vet has moved
 * on from it. `null` when there is nowhere to drive to yet.
 */
export function destinationKey(state: QuoteState): string | null {
  if (state.customer) return `customer:${state.customer.id}`
  if (state.addressDone && addressComplete(state.address)) {
    const { street, zip, city } = state.address
    return `address:${street.trim()}|${zip.trim()}|${city.trim()}`
  }
  return null
}

/** The stored quote, or an empty one when there is none or storage is out of reach. */
export function readQuote(storage: Pick<Storage, 'getItem'> | undefined): QuoteState {
  try {
    const raw = storage?.getItem(STORAGE_KEY)
    if (!raw) return EMPTY_QUOTE
    const parsed = JSON.parse(raw) as Partial<QuoteState>
    if (!Array.isArray(parsed.lines)) return EMPTY_QUOTE
    return {
      customer: parsed.customer ?? null,
      address: { ...EMPTY_QUOTE.address, ...parsed.address },
      addressDone: Boolean(parsed.addressDone),
      lines: parsed.lines,
    }
  } catch {
    return EMPTY_QUOTE
  }
}

export function writeQuote(storage: Pick<Storage, 'setItem'> | undefined, state: QuoteState) {
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    // A full or locked-down storage only costs the quote a reload.
  }
}

function sessionStore(): Storage | undefined {
  try {
    return window.sessionStorage
  } catch {
    return undefined
  }
}

/** The quote of this tab, kept across a reload. */
export function useQuote(): [QuoteState, (change: (state: QuoteState) => QuoteState) => void] {
  const [state, setState] = useState<QuoteState>(() => readQuote(sessionStore()))
  useEffect(() => writeQuote(sessionStore(), state), [state])
  return [state, setState]
}
