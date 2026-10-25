import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Pill, RefreshCw, Stethoscope, Trash2 } from 'lucide-react'
import { type FocusEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ApiError } from '@/api/fetcher'
import {
  listTemplateItems,
  priceQuote,
  quoteDistance,
  useGetCustomer,
} from '@/api/generated/endpoints'
import type {
  Customer,
  PickerItem,
  QuoteDistanceRequest,
  QuoteLine,
  QuoteLineInput,
  Template,
} from '@/api/generated/model'
import { AddressCheck } from '@/components/AddressCheck'
import { CustomerPicker } from '@/components/CustomerPicker'
import { ItemPicker } from '@/components/ItemPicker'
import { MapsLink } from '@/components/MapsLink'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/ui/field'
import { useKmToast } from '@/features/treatments/kmToast'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'
import { addressLine } from '@/lib/maps'
import { cn } from '@/lib/utils'
import {
  addPositions,
  type DraftLine,
  destinationKey,
  EMPTY_QUOTE,
  type QuoteState,
  removeLine,
  setAddress,
  setCustomer,
  updateLine,
  useQuote,
} from './quoteState'

/** How long "Neue Preisauskunft" can be taken back. */
const UNDO_MS = 10_000
/** Prices are asked a moment after the last keystroke, not on every one. */
const PRICE_DEBOUNCE_MS = 200

function customerName(customer: Customer | undefined): string | null {
  if (!customer) return null
  return [customer.first_name, customer.last_name].filter(Boolean).join(' ') || null
}

/** What the server is asked to price: the vet's choices, never an amount. */
function toPayload(lines: DraftLine[]): QuoteLineInput[] {
  return lines.map((line) => ({
    kind: line.kind,
    id: line.id,
    quantity: line.quantity,
    factor: line.factor,
    km: line.km,
    km_multiplier: line.kmMultiplier,
  }))
}

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return debounced
}

/** The line a server refusal names: `lines.3.quantity` is the fourth. */
function refusedLine(error: unknown): { index: number; message: string } | null {
  if (!(error instanceof ApiError)) return null
  for (const fieldError of error.fieldErrors) {
    const match = /^lines\.(\d+)\./.exec(fieldError.field)
    if (match) return { index: Number(match[1]), message: fieldError.message }
  }
  return null
}

/**
 * Preisauskunft: a caller asks what a treatment would cost. The vet puts the positions
 * together — for a customer, or for an address when the caller is new — and reads off the
 * gross total. Priced by the server exactly as a treatment is; nothing is saved.
 */
export function QuotePage() {
  const { t } = useTranslation()
  const { money, percent } = useLocaleFormat()
  const { maps_enabled: mapsEnabled, default_country: defaultCountry } = useOperatorConfig()
  const kmToast = useKmToast()
  const [quote, change] = useQuote()
  const destination = destinationKey(quote)

  const customer = useGetCustomer(quote.customer?.id ?? 0, {
    query: { enabled: Boolean(quote.customer) },
  })

  // Priced on the server a moment after the last change; the page adds nothing up itself.
  const payload = useDebounced(JSON.stringify(toPayload(quote.lines)), PRICE_DEBOUNCE_MS)
  const hasLines = quote.lines.length > 0
  const priced = useQuery({
    queryKey: ['price-quote', payload],
    queryFn: () => priceQuote({ lines: JSON.parse(payload) as QuoteLineInput[] }),
    enabled: payload !== '[]',
    placeholderData: keepPreviousData,
    retry: false,
  })
  const answer = hasLines ? priced.data : undefined
  const refusal = refusedLine(priced.error)
  /** A line's price, once the answer is about the same position at the same place. */
  const pricedLine = (index: number, line: DraftLine): QuoteLine | undefined => {
    const candidate = answer?.lines[index]
    return candidate && candidate.id === line.id && candidate.kind === line.kind
      ? candidate
      : undefined
  }

  // Wegegeld kilometres: asked once per line and destination, never over the vet's own.
  useEffect(() => {
    if (!destination) return
    const body: QuoteDistanceRequest = quote.customer
      ? { customer_id: quote.customer.id }
      : { ...quote.address, country: defaultCountry }
    quote.lines.forEach((line, index) => {
      const candidate = answer?.lines[index]
      const travel = candidate?.id === line.id && candidate.travel_expenses
      if (!travel || line.kmTyped || line.kmFor === destination) return
      // The kilometres of another destination are no answer for this one.
      change((state) => updateLine(state, line.key, { kmFor: destination, km: null }))
      quoteDistance(body)
        .then(({ km_suggestion: suggestion }) => {
          change((state) => {
            const current = state.lines.find((entry) => entry.key === line.key)
            // Moved on meanwhile — another destination, or typed over: this answer is stale.
            if (!current || current.kmFor !== destination || current.kmTyped) return state
            return suggestion?.km ? updateLine(state, line.key, { km: suggestion.km }) : state
          })
          kmToast(suggestion, candidate.name)
        })
        .catch(() => toast.error(t('quote.distanceFailed')))
    })
  }, [destination, quote, answer, change, defaultCountry, kmToast, t])

  const pick = (item: PickerItem) =>
    change((state) => addPositions(state, [{ kind: item.kind, id: item.id, name: item.name }]))

  const pickGroup = async (template: Template) => {
    const items = await listTemplateItems(template.id)
    change((state) =>
      addPositions(
        state,
        items.flatMap((item) => {
          const id = item.kind === 'drug_packaging' ? item.drug_packaging_id : item.service_id
          return id == null
            ? []
            : [{ kind: item.kind, id, name: item.name, quantity: item.quantity }]
        }),
      ),
    )
    toast(t('quote.groupAdded', { count: items.length, name: template.name ?? '' }))
  }

  const clear = () => {
    const previous: QuoteState = quote
    change(() => EMPTY_QUOTE)
    toast(t('quote.cleared'), {
      duration: UNDO_MS,
      action: { label: t('action.undo'), onClick: () => change(() => previous) },
    })
  }

  /** Leaving the typed address, as a whole: now it may be asked about. */
  const leaveAddress = (event: FocusEvent<HTMLFieldSetElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      change((state) => ({ ...state, addressDone: true }))
    }
  }

  const home = customer.data
    ? {
        street: customer.data.home_street,
        zip: customer.data.home_zip,
        city: customer.data.home_city,
        country: customer.data.home_country ?? defaultCountry,
      }
    : null

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        title={t('quote.title')}
        actions={
          hasLines || quote.customer || destination ? (
            <Button onClick={clear}>{t('quote.clear')}</Button>
          ) : null
        }
      >
        <p className="text-sm text-ink-soft">{t('quote.intro')}</p>
      </PageHeader>

      <section className="mt-5 rounded-card border border-line bg-surface p-4">
        <p className="eyebrow">{t('quote.forWhom')}</p>
        <p className="mt-0.5 mb-2 text-xs text-ink-faint">{t('quote.forWhomHint')}</p>
        <CustomerPicker
          customerId={quote.customer?.id ?? null}
          customerName={customerName(customer.data)}
          onPick={(id) => change((state) => setCustomer(state, { id }))}
          onClear={() => change((state) => setCustomer(state, null))}
        />
        {home ? (
          <MapsLink className="mt-1.5" address={home}>
            {addressLine(home)}
          </MapsLink>
        ) : null}

        {quote.customer ? null : (
          <>
            <fieldset className="mt-4 border-0 p-0" onBlur={leaveAddress}>
              <legend className="text-xs font-semibold text-ink-soft">
                {t('quote.addressLegend')}
              </legend>
              <div className="mt-2 grid gap-3 sm:grid-cols-4">
                <TextField
                  label={t('field.street')}
                  value={quote.address.street}
                  wrapperClassName="sm:col-span-4"
                  autoComplete="off"
                  onChange={(event) =>
                    change((state) =>
                      setAddress(state, { ...state.address, street: event.target.value }),
                    )
                  }
                />
                <TextField
                  label={t('field.zip')}
                  value={quote.address.zip}
                  inputMode="numeric"
                  autoComplete="off"
                  onChange={(event) =>
                    change((state) =>
                      setAddress(state, { ...state.address, zip: event.target.value }),
                    )
                  }
                />
                <TextField
                  label={t('field.city')}
                  value={quote.address.city}
                  wrapperClassName="sm:col-span-3"
                  autoComplete="off"
                  onChange={(event) =>
                    change((state) =>
                      setAddress(state, { ...state.address, city: event.target.value }),
                    )
                  }
                />
              </div>
            </fieldset>
            <div className="mt-2">
              <AddressCheck
                street={quote.address.street}
                zip={quote.address.zip}
                city={quote.address.city}
                country={defaultCountry}
                active={quote.addressDone}
                onApply={(suggestion) =>
                  change((state) => ({
                    ...setAddress(state, {
                      street: suggestion.street,
                      zip: suggestion.zip,
                      city: suggestion.city,
                    }),
                    addressDone: true,
                  }))
                }
              />
            </div>
          </>
        )}
      </section>

      <section className="mt-6">
        <h2 className="text-sm font-semibold text-ink">{t('quote.positions')}</h2>
        <div className="mt-2">
          <ItemPicker onPick={pick} onPickTemplate={(template) => void pickGroup(template)} />
        </div>
        {hasLines ? (
          <ul className="mt-3 flex flex-col gap-2">
            {quote.lines.map((line, index) => (
              <QuoteLineCard
                key={line.key}
                line={line}
                priced={pricedLine(index, line)}
                error={refusal?.index === index ? t(refusal.message) : undefined}
                canLookUp={mapsEnabled && destination !== null}
                onChange={(patch) => change((state) => updateLine(state, line.key, patch))}
                onLookUp={() =>
                  change((state) =>
                    updateLine(state, line.key, { km: null, kmTyped: false, kmFor: null }),
                  )
                }
                onRemove={() => change((state) => removeLine(state, line.key))}
              />
            ))}
          </ul>
        ) : (
          <p className="mt-3 rounded-card border border-dashed border-line px-3 py-6 text-center text-sm text-ink-faint">
            {t('quote.empty')}
          </p>
        )}
      </section>

      {hasLines ? (
        <section
          aria-label={t('quote.total')}
          className={cn(
            'sticky z-10 mt-4 rounded-card border border-line-strong bg-cream p-4 shadow-sm',
            // Above the phone's tab bar; on the laptop just off the bottom edge.
            'bottom-[calc(4.25rem+env(safe-area-inset-bottom))] sm:bottom-4',
          )}
        >
          <div className="flex items-baseline justify-between gap-3">
            <span className="eyebrow">{t('quote.total')}</span>
            <span className="numeric text-2xl font-semibold text-ink" aria-live="polite">
              {answer ? money(answer.total_gross) : '—'}
            </span>
          </div>
          {answer ? (
            <p className="numeric mt-1 text-right text-xs text-ink-soft">
              {[
                t('quote.net', { amount: money(answer.total_net) }),
                ...answer.vat_groups.map((group) =>
                  t('quote.vatGroup', {
                    rate: percent(group.vat_percent),
                    amount: money(group.vat),
                  }),
                ),
              ].join(' · ')}
            </p>
          ) : null}
          {priced.isError && !refusal ? (
            <p className="mt-1 text-xs text-danger" role="alert">
              {t('quote.failed')}
            </p>
          ) : null}
        </section>
      ) : null}
    </div>
  )
}

interface QuoteLineCardProps {
  line: DraftLine
  /** The server's price for it; absent while it is being asked. */
  priced: QuoteLine | undefined
  error: string | undefined
  canLookUp: boolean
  onChange: (patch: Partial<DraftLine>) => void
  /** Asks for the kilometres again. */
  onLookUp: () => void
  onRemove: () => void
}

/** One position of the quote: what it is, what it costs, and what the vet can change. */
function QuoteLineCard({
  line,
  priced,
  error,
  canLookUp,
  onChange,
  onLookUp,
  onRemove,
}: QuoteLineCardProps) {
  const { t } = useTranslation()
  const { money, percent } = useLocaleFormat()
  const Icon = line.kind === 'drug_packaging' ? Pill : Stethoscope

  return (
    <li className="rounded-card border border-line bg-surface p-3">
      <div className="flex items-start gap-2">
        <Icon
          className={cn(
            'mt-0.5 size-4 shrink-0',
            line.kind === 'drug_packaging' ? 'text-rust' : 'text-ink-soft',
          )}
          aria-hidden
        />
        <div className="min-w-0 flex-1">
          <p className="font-medium text-ink">{priced?.name ?? line.name}</p>
          {priced ? (
            <p className="text-xs text-ink-faint">
              {priced.got_number ? `GOT ${priced.got_number} · ` : ''}
              {percent(priced.vat_percent)} {t('field.vat')}
            </p>
          ) : null}
        </div>
        <span className="numeric shrink-0 font-semibold text-ink">
          {priced ? money(priced.line_gross) : '—'}
        </span>
        <Button
          size="icon"
          variant="ghost"
          aria-label={t('quote.removeLine')}
          title={t('quote.removeLine')}
          onClick={onRemove}
        >
          <Trash2 className="size-4" />
        </Button>
      </div>
      {error ? (
        <p className="mt-1 text-xs text-danger" role="alert">
          {error}
        </p>
      ) : null}

      <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
        <NumberInput
          label={t('field.quantity')}
          value={line.quantity}
          onChange={(value) => value && onChange({ quantity: value })}
        />
        {line.kind === 'service' ? (
          <NumberInput
            label={t('field.factor')}
            value={line.factor ?? priced?.factor ?? null}
            decimals={3}
            unit="%"
            onChange={(value) => onChange({ factor: value })}
          />
        ) : null}
        {priced?.travel_expenses ? (
          <>
            <NumberInput
              label={t('field.km')}
              value={line.km}
              unit="km"
              hint={line.km == null ? t('quote.kmMissing') : t('treatments.kmHint')}
              onChange={(value) => onChange({ km: value, kmTyped: value !== null })}
              action={
                canLookUp
                  ? {
                      label: t('treatments.recomputeKm'),
                      icon: <RefreshCw className="size-4" />,
                      onClick: onLookUp,
                    }
                  : undefined
              }
            />
            <NumberInput
              label={t('field.kmMultiplier')}
              value={line.kmMultiplier}
              decimals={3}
              onChange={(value) => onChange({ kmMultiplier: value })}
            />
          </>
        ) : null}
      </div>
    </li>
  )
}
