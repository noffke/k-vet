import type { Quote, QuoteLineInput, TreatmentItemKind } from '@/api/generated/model'

/**
 * What the Preisauskunft page edits, before the server has it. A quote is saved as it is put
 * together (auto-save, migration 0032); this is the page's working copy — the vet's choices,
 * never a price: prices come back from the server, which prices a quote as it prices a
 * treatment.
 */

/** One position as the vet set it. */
export interface EditableLine {
  /** Stable while the line is on the page, for React and for editing one line. */
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
  /** The vet typed the kilometres herself, or they were already there: never overwritten. */
  kmTyped: boolean
  /** The destination the kilometres were last looked up for, so each is asked once. */
  kmFor: string | null
}

export interface QuoteAddress {
  street: string
  zip: string
  city: string
}

export interface QuoteDraft {
  customerId: number | null
  name: string
  note: string
  /** A new caller's address, for the Wegegeld. */
  address: QuoteAddress
  /** The vet has finished typing the address and moved on: only then is it asked about. */
  addressDone: boolean
  lines: EditableLine[]
}

export const EMPTY_DRAFT: QuoteDraft = {
  customerId: null,
  name: '',
  note: '',
  address: { street: '', zip: '', city: '' },
  addressDone: false,
  lines: [],
}

let counter = 0
/** `crypto.randomUUID` needs a secure context, which a practice LAN on plain HTTP is not. */
function newKey(): string {
  counter += 1
  return `${Date.now().toString(36)}-${counter}`
}

/** The page's copy of a stored quote. Kilometres already there count as settled. */
export function fromQuote(quote: Quote): QuoteDraft {
  const draft: QuoteDraft = {
    customerId: quote.customer_id ?? null,
    name: quote.name ?? '',
    note: quote.note ?? '',
    address: {
      street: quote.address_street ?? '',
      zip: quote.address_zip ?? '',
      city: quote.address_city ?? '',
    },
    addressDone: true,
    lines: [],
  }
  const destination = destinationKey(draft)
  draft.lines = quote.lines.map((line) => ({
    key: newKey(),
    kind: line.kind,
    id: line.id,
    name: line.name,
    quantity: line.quantity,
    factor: line.factor ?? null,
    km: line.km ?? null,
    kmMultiplier: line.km_multiplier ?? null,
    kmTyped: line.km != null,
    kmFor: destination,
  }))
  return draft
}

/** What a picked position or a group's entry brings along. */
export interface PickedPosition {
  kind: TreatmentItemKind
  id: number
  name: string
  quantity?: string
}

export function addPositions(draft: QuoteDraft, picked: PickedPosition[]): QuoteDraft {
  const lines = picked.map(
    (position): EditableLine => ({
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
  return { ...draft, lines: [...draft.lines, ...lines] }
}

export function updateLine(
  draft: QuoteDraft,
  key: string,
  patch: Partial<EditableLine>,
): QuoteDraft {
  return {
    ...draft,
    lines: draft.lines.map((line) => (line.key === key ? { ...line, ...patch } : line)),
  }
}

export function removeLine(draft: QuoteDraft, key: string): QuoteDraft {
  return { ...draft, lines: draft.lines.filter((line) => line.key !== key) }
}

/** A customer replaces a typed address: the drive goes to their home. */
export function setCustomer(draft: QuoteDraft, customerId: number | null): QuoteDraft {
  return { ...draft, customerId, address: EMPTY_DRAFT.address, addressDone: false }
}

/** Typing an address is for a caller who is not a customer yet. */
export function setAddress(draft: QuoteDraft, address: QuoteAddress): QuoteDraft {
  return { ...draft, customerId: null, address, addressDone: false }
}

function addressComplete(address: QuoteAddress): boolean {
  return [address.street, address.zip, address.city].every((part) => part.trim() !== '')
}

/**
 * Where the Wegegeld goes, as one key: the customer, or a typed address once the vet has moved
 * on from it. `null` when there is nowhere to drive to yet.
 */
export function destinationKey(draft: QuoteDraft): string | null {
  if (draft.customerId !== null) return `customer:${draft.customerId}`
  if (draft.addressDone && addressComplete(draft.address)) {
    const { street, zip, city } = draft.address
    return `address:${street.trim()}|${zip.trim()}|${city.trim()}`
  }
  return null
}

/** Whether there is anything worth saving yet: a position, a name, a note or an address. */
export function hasContent(draft: QuoteDraft): boolean {
  const { street, zip, city } = draft.address
  return (
    draft.lines.length > 0 ||
    [draft.name, draft.note, street, zip, city].some((text) => text.trim() !== '')
  )
}

/** What the server is asked to price: the vet's choices, never an amount. */
export function toLineInputs(lines: EditableLine[]): QuoteLineInput[] {
  return lines.map((line) => ({
    kind: line.kind,
    id: line.id,
    quantity: line.quantity,
    factor: line.factor,
    km: line.km,
    km_multiplier: line.kmMultiplier,
  }))
}
