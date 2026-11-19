import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { PatchQuote, PickerItem, Quote } from '@/api/generated/model'
import { QuotePage } from '@/features/quote/QuotePage'
import i18n from '@/lib/i18n'

/** The record's creation, answered when the test says so. */
let created: (quote: { id: number }) => void = () => undefined
const createQuote = vi.fn(
  () =>
    new Promise<{ id: number }>((resolve) => {
      created = resolve
    }),
)
const patchQuote = vi.fn(async (id: number, _data: PatchQuote) => ({ id }) as Quote)

vi.mock('@/api/generated/endpoints', () => ({
  createQuote: () => createQuote(),
  patchQuote: (id: number, data: PatchQuote) => patchQuote(id, data),
  createCustomer: vi.fn(),
  deleteQuote: vi.fn(),
  listTemplateItems: vi.fn(),
  patchCustomer: vi.fn(),
  quoteDistance: vi.fn(),
  getGetQuoteQueryKey: (id: number) => ['quote', id],
  getListCustomerQuotesQueryKey: (id: number) => ['customer-quotes', id],
  getListOpenQuotesQueryKey: () => ['open-quotes'],
  useGetQuote: () => ({ data: undefined }),
  useGetCustomer: () => ({ data: undefined }),
  useGetConfig: () => ({ data: undefined }),
}))
vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => vi.fn(async () => undefined),
  useSearch: () => ({}),
  useBlocker: () => ({ status: 'idle' }),
}))
vi.mock('@/lib/config', () => ({
  useOperatorConfig: () => ({ maps_enabled: false, default_country: 'DE' }),
}))
vi.mock('@/features/treatments/kmToast', () => ({ useKmToast: () => vi.fn() }))
// The page around the save: the header, the pickers and the dialogs stand in as little as the
// test needs — one button that picks a position.
vi.mock('@/components/PageHeader', () => ({ PageHeader: () => null }))
vi.mock('@/components/CustomerPicker', () => ({ CustomerPicker: () => null }))
vi.mock('@/components/AddressCheck', () => ({ AddressCheck: () => null }))
vi.mock('@/components/MapsLink', () => ({ MapsLink: () => null }))
vi.mock('@/features/quote/OpenQuotes', () => ({ OpenQuotes: () => null }))
vi.mock('@/features/quote/KeepQuoteDialog', () => ({ KeepQuoteDialog: () => null }))
vi.mock('@/components/ItemPicker', () => ({
  ItemPicker: ({ onPick }: { onPick: (item: PickerItem) => void }) => (
    <button
      type="button"
      onClick={() =>
        onPick({ kind: 'service', id: 7, name: 'Allgemeine Untersuchung' } as unknown as PickerItem)
      }
    >
      pick
    </button>
  ),
}))

beforeAll(async () => {
  await i18n.changeLanguage('de')
})

describe('saving a Preisauskunft', () => {
  it('saves a position picked while the first save is still creating the record', async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <QuotePage />
      </QueryClientProvider>,
    )

    // The address is typed: a moment later the first save starts creating the record.
    fireEvent.change(screen.getByLabelText('Straße und Hausnummer'), {
      target: { value: 'Musterweg 5' },
    })
    await waitFor(() => expect(createQuote).toHaveBeenCalledTimes(1))

    // A position is picked before the server has answered.
    fireEvent.click(screen.getByRole('button', { name: 'pick' }))
    created({ id: 2 })

    // The first save writes what it was started with; the position is the next save's.
    await waitFor(() => expect(patchQuote).toHaveBeenCalledTimes(2), { timeout: 2000 })
    expect(patchQuote.mock.calls[0]?.[1]).toMatchObject({ address_street: 'Musterweg 5' })
    expect(patchQuote.mock.calls[0]?.[1].lines).toBeUndefined()
    expect(patchQuote.mock.calls[1]?.[0]).toBe(2)
    expect(patchQuote.mock.calls[1]?.[1].lines).toEqual([
      expect.objectContaining({ kind: 'service', id: 7 }),
    ])
    expect(createQuote).toHaveBeenCalledTimes(1)
  })
})
