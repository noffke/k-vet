import { describe, expect, it } from 'vitest'
import type { Drug, Packaging, ScanResult } from '@/api/generated/model'
import { type Delivery, deliveryReducer, emptyDelivery } from '@/features/pharmacy/delivery'

const metacam = { id: 7, name: 'Metacam 10 ml', draft: false } as Drug
const amoxi = { id: 8, name: 'Amoxicillin', draft: false } as Drug
const original = {
  id: 70,
  kind: 'original',
  quantity: '10.00',
  unit: 'ml',
  draft: false,
} as Packaging

function scanned(result: Partial<ScanResult>): ScanResult {
  return {
    gtin: null,
    display: null,
    batch: null,
    expiry: null,
    drug: null,
    original: null,
    ...result,
  } as ScanResult
}

const ean = (drug: Drug | null) =>
  scanned({
    gtin: '04006381333931',
    display: '4006381333931',
    drug,
    original: drug ? original : null,
  })
const matrix = (drug: Drug | null, batch: string) =>
  scanned({
    gtin: '04150123456782',
    display: 'PZN 12345678',
    batch,
    expiry: '2029-08-31',
    drug,
    original: drug ? original : null,
  })

function run(...results: ScanResult[]): Delivery {
  return results.reduce(
    (state, result) => deliveryReducer(state, { type: 'scanned', raw: 'raw', result }),
    emptyDelivery,
  )
}

/**
 * What a scan means for a delivery being booked pack by pack (the delivery page): the pack in
 * hand, its second code, one more of the same, another batch, another drug, a code no drug has.
 */
describe('deliveryReducer', () => {
  it('makes a known pack the one in hand, with what its scan carried', () => {
    const state = run(matrix(metacam, 'AB12'))
    expect(state.current?.drug.id).toBe(7)
    expect(state.current?.intake).toMatchObject({
      packages: '1',
      batch: 'AB12',
      expiry: '2029-08-31',
      price: { state: 'open' },
    })
  })

  it("takes the pack's Data Matrix after its EAN as the same pack, and further scans as more", () => {
    const second = run(ean(metacam), matrix(metacam, 'AB12'))
    expect(second.current?.intake).toMatchObject({ packages: '1', batch: 'AB12' })

    const more = run(ean(metacam), matrix(metacam, 'AB12'), ean(metacam), matrix(metacam, 'AB12'))
    expect(more.current?.intake.packages).toBe('3')
  })

  it('keeps another batch and another drug waiting until the pack in hand is booked', () => {
    const otherBatch = run(matrix(metacam, 'AB12'), matrix(metacam, 'ZZ99'))
    expect(otherBatch.notice?.key).toBe('delivery.otherBatch')
    expect(otherBatch.current?.intake).toMatchObject({ packages: '1', batch: 'AB12' })

    const otherDrug = run(ean(metacam), ean(amoxi))
    expect(otherDrug.notice).toEqual({
      key: 'delivery.finishCurrent',
      values: { name: 'Metacam 10 ml' },
    })
    expect(otherDrug.current?.drug.id).toBe(7)
  })

  it('holds a code no drug has — or, with a pack in hand, takes it as that pack’s other code', () => {
    const unknown = run(ean(null))
    expect(unknown.unknown).toEqual({ raw: 'raw', display: '4006381333931' })
    expect(unknown.current).toBeNull()

    const otherCode = run(ean(metacam), matrix(null, 'AB12'))
    expect(otherCode.current?.intake.batch).toBe('AB12')
    expect(otherCode.current?.newCode).toEqual({ raw: 'raw', display: 'PZN 12345678' })
  })

  it('asks the list price once per drug in a delivery, and lists what is booked', () => {
    let state = run(ean(metacam))
    state = deliveryReducer(state, {
      type: 'edited',
      intake: { ...(state.current?.intake ?? ({} as never)), price: { state: 'same' } },
    })
    state = deliveryReducer(state, { type: 'booked', lotId: 501 })
    expect(state.current).toBeNull()
    expect(state.booked).toEqual([
      { lotId: 501, drugName: 'Metacam 10 ml', batch: null, expiry: null, packages: 1 },
    ])

    state = deliveryReducer(state, { type: 'scanned', raw: 'raw', result: ean(metacam) })
    expect(state.current?.intake.price).toEqual({ state: 'earlier' })
    state = deliveryReducer(state, { type: 'discarded' })
    state = deliveryReducer(state, { type: 'scanned', raw: 'raw', result: ean(amoxi) })
    expect(state.current?.intake.price).toEqual({ state: 'open' })

    state = deliveryReducer(state, { type: 'undone', lotId: 501 })
    expect(state.booked).toEqual([])
  })

  it('refuses what cannot be booked or read', () => {
    expect(run(scanned({ gtin: null })).notice?.key).toBe('code.invalid')
    const noOriginal = run(scanned({ gtin: '04006381333931', drug: metacam, original: null }))
    expect(noOriginal.notice?.key).toBe('delivery.notStockable')
    expect(noOriginal.current).toBeNull()
    expect(run(scanned({ batch: 'AB12' })).notice?.key).toBe('delivery.scanProductFirst')
  })
})
