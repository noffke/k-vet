import { describe, expect, it } from 'vitest'
import {
  addPositions,
  destinationKey,
  EMPTY_QUOTE,
  readQuote,
  removeLine,
  setAddress,
  setCustomer,
  updateLine,
  writeQuote,
} from '@/features/quote/quoteState'

const service = { kind: 'service' as const, id: 7, name: 'Allgemeine Untersuchung' }
const drug = {
  kind: 'drug_packaging' as const,
  id: 3,
  name: 'Amoxicillin · 10 ml',
  quantity: '2.00',
}

describe('a quote', () => {
  it('takes positions with their own quantity, or one', () => {
    const quote = addPositions(EMPTY_QUOTE, [service, drug])
    expect(quote.lines.map((line) => [line.kind, line.id, line.quantity])).toEqual([
      ['service', 7, '1.00'],
      ['drug_packaging', 3, '2.00'],
    ])
    expect(new Set(quote.lines.map((line) => line.key)).size).toBe(2)
    expect(quote.lines[0]).toMatchObject({ factor: null, km: null, kmTyped: false, kmFor: null })
  })

  it('changes and removes one line, leaving the others', () => {
    const quote = addPositions(EMPTY_QUOTE, [service, drug])
    const [first, second] = quote.lines
    if (!first || !second) throw new Error('two lines expected')
    const changed = updateLine(quote, first.key, { factor: '230.000' })
    expect(changed.lines[0]?.factor).toBe('230.000')
    expect(changed.lines[1]).toEqual(second)
    expect(removeLine(changed, first.key).lines).toEqual([second])
  })

  it('drives either to a customer or to a typed address, never both', () => {
    const typed = setAddress(EMPTY_QUOTE, { street: 'Weg 1', zip: '12345', city: 'Stadt' })
    // Still typing: nowhere to drive to yet.
    expect(destinationKey(typed)).toBeNull()
    const done = { ...typed, addressDone: true }
    expect(destinationKey(done)).toBe('address:Weg 1|12345|Stadt')
    expect(destinationKey({ ...done, address: { ...done.address, city: ' ' } })).toBeNull()

    const customer = setCustomer(done, { id: 42 })
    expect(customer.address).toEqual(EMPTY_QUOTE.address)
    expect(destinationKey(customer)).toBe('customer:42')
    expect(setAddress(customer, { street: 'Weg 2', zip: '', city: '' }).customer).toBeNull()
  })

  it('survives a reload through the storage it is given', () => {
    const store = new Map<string, string>()
    const storage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    }
    const quote = addPositions(setCustomer(EMPTY_QUOTE, { id: 5 }), [service])
    writeQuote(storage, quote)
    expect(readQuote(storage)).toEqual(quote)
  })

  it('starts empty when storage is missing, broken or out of reach', () => {
    expect(readQuote(undefined)).toEqual(EMPTY_QUOTE)
    expect(readQuote({ getItem: () => '{not json' })).toEqual(EMPTY_QUOTE)
    expect(readQuote({ getItem: () => '{"lines": "nope"}' })).toEqual(EMPTY_QUOTE)
    expect(
      readQuote({
        getItem: () => {
          throw new Error('denied')
        },
      }),
    ).toEqual(EMPTY_QUOTE)
    expect(() =>
      writeQuote(
        {
          setItem: () => {
            throw new Error('full')
          },
        },
        EMPTY_QUOTE,
      ),
    ).not.toThrow()
  })
})
