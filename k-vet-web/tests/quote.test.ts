import { describe, expect, it } from 'vitest'
import type { Quote } from '@/api/generated/model'
import {
  addPositions,
  destinationKey,
  EMPTY_DRAFT,
  fromQuote,
  hasContent,
  removeLine,
  setAddress,
  setCustomer,
  toLineInputs,
  updateLine,
} from '@/features/quote/quoteState'

const service = { kind: 'service' as const, id: 7, name: 'Allgemeine Untersuchung' }
const drug = {
  kind: 'drug_packaging' as const,
  id: 3,
  name: 'Amoxicillin · 10 ml',
  quantity: '2.00',
}

describe('a quote being put together', () => {
  it('takes positions with their own quantity, or one', () => {
    const draft = addPositions(EMPTY_DRAFT, [service, drug])
    expect(draft.lines.map((line) => [line.kind, line.id, line.quantity])).toEqual([
      ['service', 7, '1.00'],
      ['drug_packaging', 3, '2.00'],
    ])
    expect(new Set(draft.lines.map((line) => line.key)).size).toBe(2)
    expect(draft.lines[0]).toMatchObject({ factor: null, km: null, kmTyped: false, kmFor: null })
  })

  it('changes and removes one line, leaving the others', () => {
    const draft = addPositions(EMPTY_DRAFT, [service, drug])
    const [first, second] = draft.lines
    if (!first || !second) throw new Error('two lines expected')
    const changed = updateLine(draft, first.key, { factor: '230.000' })
    expect(changed.lines[0]?.factor).toBe('230.000')
    expect(changed.lines[1]).toEqual(second)
    expect(removeLine(changed, first.key).lines).toEqual([second])
  })

  it('sends the vet’s choices, never an amount', () => {
    const draft = addPositions(EMPTY_DRAFT, [service])
    const [line] = draft.lines
    if (!line) throw new Error('a line expected')
    const inputs = toLineInputs(updateLine(draft, line.key, { km: '12', kmMultiplier: '2' }).lines)
    expect(inputs).toEqual([
      { kind: 'service', id: 7, quantity: '1.00', factor: null, km: '12', km_multiplier: '2' },
    ])
  })

  it('drives either to a customer or to a typed address, never both', () => {
    const typed = setAddress(EMPTY_DRAFT, { street: 'Weg 1', zip: '12345', city: 'Stadt' })
    // Still typing: nowhere to drive to yet.
    expect(destinationKey(typed)).toBeNull()
    const done = { ...typed, addressDone: true }
    expect(destinationKey(done)).toBe('address:Weg 1|12345|Stadt')
    expect(destinationKey({ ...done, address: { ...done.address, city: ' ' } })).toBeNull()

    const customer = setCustomer(done, 42)
    expect(customer.address).toEqual(EMPTY_DRAFT.address)
    expect(destinationKey(customer)).toBe('customer:42')
    expect(setAddress(customer, { street: 'Weg 2', zip: '', city: '' }).customerId).toBeNull()
  })

  it('is worth saving once anything is in it', () => {
    expect(hasContent(EMPTY_DRAFT)).toBe(false)
    expect(hasContent({ ...EMPTY_DRAFT, name: '  ' })).toBe(false)
    expect(hasContent({ ...EMPTY_DRAFT, note: 'ruft zurück' })).toBe(true)
    expect(hasContent(addPositions(EMPTY_DRAFT, [service]))).toBe(true)
  })

  it('reopens a stored quote with its kilometres settled', () => {
    const stored = {
      id: 5,
      customer_id: 42,
      name: 'Kastration',
      note: null,
      address_street: null,
      address_zip: null,
      address_city: null,
      lines: [
        {
          kind: 'service',
          id: 9,
          name: 'Wegegeld',
          quantity: '1.00',
          factor: null,
          km: '12.00',
          km_multiplier: null,
        },
        {
          kind: 'service',
          id: 7,
          name: 'Untersuchung',
          quantity: '1.00',
          factor: '230.000',
          km: null,
          km_multiplier: null,
        },
      ],
    } as unknown as Quote
    const draft = fromQuote(stored)
    expect(draft).toMatchObject({ customerId: 42, name: 'Kastration', note: '' })
    expect(draft.lines[0]).toMatchObject({ km: '12.00', kmTyped: true, kmFor: 'customer:42' })
    expect(draft.lines[1]).toMatchObject({ factor: '230.000', kmTyped: false })
  })
})
