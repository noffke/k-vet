import { useQueryClient } from '@tanstack/react-query'
import { useBlocker, useNavigate, useSearch } from '@tanstack/react-router'
import { Pill, RefreshCw, Stethoscope, Trash2 } from 'lucide-react'
import { type FocusEvent, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ApiError } from '@/api/fetcher'
import {
  createCustomer,
  createQuote,
  deleteQuote,
  getGetQuoteQueryKey,
  getListCustomerQuotesQueryKey,
  getListOpenQuotesQueryKey,
  listTemplateItems,
  patchCustomer,
  patchQuote,
  quoteDistance,
  useGetCustomer,
  useGetQuote,
} from '@/api/generated/endpoints'
import type {
  PatchQuote,
  PickerItem,
  Quote,
  QuoteDistanceRequest,
  QuoteLine,
  Template,
} from '@/api/generated/model'
import { AddressCheck } from '@/components/AddressCheck'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { CustomerPicker } from '@/components/CustomerPicker'
import { ItemPicker } from '@/components/ItemPicker'
import { MapsLink } from '@/components/MapsLink'
import { NumberInput } from '@/components/NumberInput'
import { PageHeader } from '@/components/PageHeader'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { TextAreaField, TextField } from '@/components/ui/field'
import { useKmToast } from '@/features/treatments/kmToast'
import type { SaveState } from '@/lib/autosave'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'
import { addressLine } from '@/lib/maps'
import { cn } from '@/lib/utils'
import { type KeepChoice, KeepQuoteDialog } from './KeepQuoteDialog'
import { OpenQuotes } from './OpenQuotes'
import {
  addPositions,
  destinationKey,
  type EditableLine,
  EMPTY_DRAFT,
  fromQuote,
  hasContent,
  type QuoteDraft,
  removeLine,
  setAddress,
  setCustomer,
  toLineInputs,
  updateLine,
} from './quoteState'

/** Saved a moment after the last keystroke, as every record is. */
const SAVE_DEBOUNCE_MS = 400

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
 * gross total. Saved as she types (migration 0032): a quote with a customer is kept on their
 * page; one without stays open, and leaving it asks whether to keep or discard it.
 */
export function QuotePage() {
  const { t } = useTranslation()
  const { money } = useLocaleFormat()
  const { maps_enabled: mapsEnabled, default_country: defaultCountry } = useOperatorConfig()
  const kmToast = useKmToast()
  const client = useQueryClient()
  const navigate = useNavigate()
  const { id } = useSearch({ from: '/app/quote' })

  const quote = useGetQuote(id ?? 0, { query: { enabled: id !== undefined } })
  const [draft, setDraft] = useState<QuoteDraft>(EMPTY_DRAFT)
  const draftRef = useRef(draft)
  draftRef.current = draft
  /** The quote the draft was taken from: a save's answer never overwrites what is typed. */
  const [loadedFor, setLoadedFor] = useState<number | undefined>(undefined)
  const quoteId = useRef<number | undefined>(id)
  const dirty = useRef({ fields: false, lines: false })
  const chain = useRef<Promise<void>>(Promise.resolve())
  const [saveState, setSaveState] = useState<SaveState>('idle')
  const [saveError, setSaveError] = useState<unknown>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)
  /** Set before a navigation that must not ask — after the quote was just deleted. */
  const skipBlock = useRef(false)

  // A quote opened by its id becomes the page's draft, once; leaving one for "Neue
  // Preisauskunft" starts afresh. Only the step from an id to none is such a leaving: right
  // after the first save created the quote, the address does not carry its id yet, and taking
  // that for a new quote wiped what was just typed and forgot the record it had gone into.
  const previousId = useRef(id)
  useEffect(() => {
    const before = previousId.current
    previousId.current = id
    if (id === undefined) {
      if (before !== undefined) {
        quoteId.current = undefined
        setDraft(EMPTY_DRAFT)
        setLoadedFor(undefined)
        setSaveState('idle')
      }
      return
    }
    quoteId.current = id
    if (loadedFor !== id && quote.data?.id === id) {
      setDraft(fromQuote(quote.data))
      setLoadedFor(id)
    }
  }, [id, quote.data, loadedFor])

  const edit = (change: (current: QuoteDraft) => QuoteDraft, what: 'fields' | 'lines') => {
    setDraft(change)
    dirty.current[what] = true
  }

  /** Writes what changed; the first save creates the record. Saves run one after another. */
  const save = useCallback((): Promise<void> => {
    chain.current = chain.current.then(async () => {
      const current = draftRef.current
      const { fields, lines } = dirty.current
      if (!fields && !lines) return
      let target = quoteId.current
      if (target === undefined) {
        if (!hasContent(current)) return
        const created = await createQuote({ customer_id: null })
        target = created.id
        quoteId.current = target
        setLoadedFor(target)
        void navigate({ to: '/quote', search: { id: target }, replace: true })
      }
      dirty.current = { fields: false, lines: false }
      const data: PatchQuote =
        current.customerId !== null
          ? { customer_id: current.customerId }
          : {
              address_street: current.address.street.trim() || null,
              address_zip: current.address.zip.trim() || null,
              address_city: current.address.city.trim() || null,
            }
      data.name = current.name.trim() || null
      data.note = current.note.trim() || null
      // Lines only when they changed: saving them prices them at today's catalogue, and a
      // quote reopened just to fix its note keeps what it was quoted at.
      if (lines) data.lines = toLineInputs(current.lines)
      setSaveState('saving')
      try {
        const saved = await patchQuote(target, data)
        client.setQueryData(getGetQuoteQueryKey(target), saved)
        setSaveError(null)
        setSaveState('saved')
      } catch (error) {
        setSaveError(error)
        setSaveState('error')
      }
      void client.invalidateQueries({ queryKey: getListOpenQuotesQueryKey() })
    })
    return chain.current
  }, [client, navigate])

  // Saved a moment after the last change.
  const fingerprint = JSON.stringify({
    customerId: draft.customerId,
    name: draft.name,
    note: draft.note,
    address: draft.address,
    lines: toLineInputs(draft.lines),
  })
  // biome-ignore lint/correctness/useExhaustiveDependencies: the timer restarts on every change of the content, which `fingerprint` stands for; the save reads the latest draft itself
  useEffect(() => {
    if (!dirty.current.fields && !dirty.current.lines) return
    const timer = setTimeout(() => void save(), SAVE_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [fingerprint, save])

  const answer = id !== undefined && quote.data?.id === id ? quote.data : undefined
  const kept = answer ? !answer.draft : false
  /** Still open — no customer — with positions on it: leaving asks keep or discard. */
  const open = draft.customerId === null && draft.lines.length > 0 && !kept
  const blocker = useBlocker({
    shouldBlockFn: ({ next }) => {
      if (skipBlock.current) {
        skipBlock.current = false
        return false
      }
      if (!open) return false
      const nextId = (next.search as { id?: number }).id
      return !(next.pathname === '/quote' && nextId !== undefined && nextId === quoteId.current)
    },
    // Closing the tab loses nothing: the quote is saved and listed as open.
    enableBeforeUnload: false,
    withResolver: true,
  })

  const destination = destinationKey(draft)
  const customer = useGetCustomer(draft.customerId ?? 0, {
    query: { enabled: draft.customerId !== null },
  })
  const refusal = refusedLine(saveError)
  /** A line's price, once the answer is about the same position at the same place. */
  const pricedLine = (index: number, line: EditableLine): QuoteLine | undefined => {
    const candidate = answer?.lines[index]
    return candidate && candidate.id === line.id && candidate.kind === line.kind
      ? candidate
      : undefined
  }

  // Wegegeld kilometres: asked once per line and destination, never over the vet's own.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `edit` and `kmToast` are recreated each render and change nothing about what to ask
  useEffect(() => {
    if (!destination) return
    const body: QuoteDistanceRequest =
      draft.customerId !== null
        ? { customer_id: draft.customerId }
        : { ...draft.address, country: defaultCountry }
    draft.lines.forEach((line, index) => {
      const candidate = answer?.lines[index]
      const travel = candidate?.id === line.id && candidate.travel_expenses
      if (!travel || line.kmTyped || line.kmFor === destination) return
      // The kilometres of another destination are no answer for this one.
      edit((current) => updateLine(current, line.key, { kmFor: destination, km: null }), 'lines')
      quoteDistance(body)
        .then(({ km_suggestion: suggestion }) => {
          const current = draftRef.current.lines.find((entry) => entry.key === line.key)
          // Moved on meanwhile — another destination, or typed over: this answer is stale.
          if (!current || current.kmFor !== destination || current.kmTyped) return
          if (suggestion?.km) {
            const km = suggestion.km
            edit((state) => updateLine(state, line.key, { km }), 'lines')
          }
          kmToast(suggestion, candidate.name)
        })
        .catch(() => toast.error(t('quote.distanceFailed')))
    })
  }, [destination, draft, answer, defaultCountry, t])

  const pick = (item: PickerItem) =>
    edit(
      (current) => addPositions(current, [{ kind: item.kind, id: item.id, name: item.name }]),
      'lines',
    )

  const pickGroup = async (template: Template) => {
    const items = await listTemplateItems(template.id)
    edit(
      (current) =>
        addPositions(
          current,
          items.flatMap((item) => {
            const id = item.kind === 'drug_packaging' ? item.drug_packaging_id : item.service_id
            return id == null
              ? []
              : [{ kind: item.kind, id, name: item.name, quantity: item.quantity }]
          }),
        ),
      'lines',
    )
    toast(t('quote.groupAdded', { count: items.length, name: template.name ?? '' }))
  }

  /** Leaving the typed address, as a whole: now it may be asked about. */
  const leaveAddress = (event: FocusEvent<HTMLFieldSetElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      setDraft((current) => ({ ...current, addressDone: true }))
    }
  }

  const keep = async (choice: KeepChoice) => {
    let customerId = draftRef.current.customerId
    if (choice.customer.kind === 'existing') {
      customerId = choice.customer.id
    } else if (choice.customer.kind === 'new') {
      const created = await createCustomer()
      const { street, zip, city } = draftRef.current.address
      await patchCustomer(created.id, {
        salutation: choice.customer.salutation,
        first_name: choice.customer.firstName.trim() || null,
        last_name: choice.customer.lastName.trim() || null,
        home_street: street.trim() || null,
        home_zip: zip.trim() || null,
        home_city: city.trim() || null,
      })
      customerId = created.id
    }
    const next = { ...draftRef.current, customerId, name: choice.name }
    draftRef.current = next
    setDraft(next)
    dirty.current.fields = true
    await save()
    if (customerId !== null) {
      void client.invalidateQueries({ queryKey: getListCustomerQuotesQueryKey(customerId) })
    }
    toast(t('quote.kept'))
  }

  const discard = async () => {
    const target = quoteId.current
    if (target !== undefined) {
      await chain.current
      await deleteQuote(target)
      void client.invalidateQueries({ queryKey: getListOpenQuotesQueryKey() })
    }
    dirty.current = { fields: false, lines: false }
  }

  const home = customer.data
    ? {
        street: customer.data.home_street,
        zip: customer.data.home_zip,
        city: customer.data.home_city,
        country: customer.data.home_country ?? defaultCountry,
      }
    : null
  const customerName =
    answer?.customer_name ??
    ([customer.data?.first_name, customer.data?.last_name].filter(Boolean).join(' ') || null)

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        title={t('quote.title')}
        actions={
          <>
            {id !== undefined ? <SaveIndicator state={saveState} error={null} /> : null}
            {id !== undefined || hasContent(draft) ? (
              <Button onClick={() => void navigate({ to: '/quote', search: {} })}>
                {t('quote.new')}
              </Button>
            ) : null}
            {id !== undefined ? (
              <Button variant="danger" onClick={() => setConfirmDelete(true)}>
                <Trash2 className="size-4" />
                {t('action.delete')}
              </Button>
            ) : null}
          </>
        }
      >
        <p className="text-sm text-ink-soft">{t('quote.intro')}</p>
      </PageHeader>

      <section className="mt-5 grid gap-3 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <TextField
          label={t('quote.name')}
          value={draft.name}
          placeholder={t('quote.namePlaceholder')}
          onChange={(event) => {
            const name = event.target.value
            edit((current) => ({ ...current, name }), 'fields')
          }}
        />
        <TextAreaField
          label={t('quote.note')}
          value={draft.note}
          wrapperClassName="sm:col-span-2"
          onChange={(event) => {
            const note = event.target.value
            edit((current) => ({ ...current, note }), 'fields')
          }}
        />
      </section>

      <section className="mt-4 rounded-card border border-line bg-surface p-4">
        <p className="eyebrow">{t('quote.forWhom')}</p>
        <p className="mt-0.5 mb-2 text-xs text-ink-faint">{t('quote.forWhomHint')}</p>
        <CustomerPicker
          customerId={draft.customerId}
          customerName={customerName}
          // A kept quote stays its customer's.
          locked={kept}
          onPick={(customerId) => edit((current) => setCustomer(current, customerId), 'fields')}
          onClear={() => edit((current) => setCustomer(current, null), 'fields')}
        />
        {home ? (
          <MapsLink className="mt-1.5" address={home}>
            {addressLine(home)}
          </MapsLink>
        ) : null}

        {draft.customerId !== null ? null : (
          <>
            <fieldset className="mt-4 border-0 p-0" onBlur={leaveAddress}>
              <legend className="text-xs font-semibold text-ink-soft">
                {t('quote.addressLegend')}
              </legend>
              <div className="mt-2 grid gap-3 sm:grid-cols-4">
                <TextField
                  label={t('field.street')}
                  value={draft.address.street}
                  wrapperClassName="sm:col-span-4"
                  autoComplete="off"
                  onChange={(event) => {
                    const street = event.target.value
                    edit((current) => setAddress(current, { ...current.address, street }), 'fields')
                  }}
                />
                <TextField
                  label={t('field.zip')}
                  value={draft.address.zip}
                  inputMode="numeric"
                  autoComplete="off"
                  onChange={(event) => {
                    const zip = event.target.value
                    edit((current) => setAddress(current, { ...current.address, zip }), 'fields')
                  }}
                />
                <TextField
                  label={t('field.city')}
                  value={draft.address.city}
                  wrapperClassName="sm:col-span-3"
                  autoComplete="off"
                  onChange={(event) => {
                    const city = event.target.value
                    edit((current) => setAddress(current, { ...current.address, city }), 'fields')
                  }}
                />
              </div>
            </fieldset>
            <div className="mt-2">
              <AddressCheck
                street={draft.address.street}
                zip={draft.address.zip}
                city={draft.address.city}
                country={defaultCountry}
                active={draft.addressDone}
                onApply={(suggestion) =>
                  edit(
                    (current) => ({
                      ...setAddress(current, {
                        street: suggestion.street,
                        zip: suggestion.zip,
                        city: suggestion.city,
                      }),
                      addressDone: true,
                    }),
                    'fields',
                  )
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
        {draft.lines.length > 0 ? (
          <ul className="mt-3 flex flex-col gap-2">
            {draft.lines.map((line, index) => (
              <QuoteLineCard
                key={line.key}
                line={line}
                priced={pricedLine(index, line)}
                error={refusal?.index === index ? t(refusal.message) : undefined}
                canLookUp={mapsEnabled && destination !== null}
                onChange={(patch) =>
                  edit((current) => updateLine(current, line.key, patch), 'lines')
                }
                onLookUp={() =>
                  edit(
                    (current) =>
                      updateLine(current, line.key, { km: null, kmTyped: false, kmFor: null }),
                    'lines',
                  )
                }
                onRemove={() => edit((current) => removeLine(current, line.key), 'lines')}
              />
            ))}
          </ul>
        ) : (
          <p className="mt-3 rounded-card border border-dashed border-line px-3 py-6 text-center text-sm text-ink-faint">
            {t('quote.empty')}
          </p>
        )}
      </section>

      {draft.lines.length > 0 ? (
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
          {answer ? <TotalDetails quote={answer} /> : null}
          {saveState === 'error' && !refusal ? (
            <p className="mt-1 text-xs text-danger" role="alert">
              {t('quote.failed')}
            </p>
          ) : null}
        </section>
      ) : null}

      <OpenQuotes exclude={id} />

      <KeepQuoteDialog
        open={blocker.status === 'blocked'}
        name={draft.name}
        customerId={draft.customerId}
        address={draft.address}
        onKeep={async (choice) => {
          await keep(choice)
          blocker.proceed?.()
        }}
        onDiscard={async () => {
          await discard()
          blocker.proceed?.()
        }}
        onCancel={() => blocker.reset?.()}
      />
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={t('quote.deleteTitle')}
        confirmLabel={t('action.delete')}
        onConfirm={async () => {
          await discard()
          setConfirmDelete(false)
          skipBlock.current = true
          await navigate({ to: '/quote', search: {} })
        }}
      >
        <p className="text-sm text-ink-soft">{t('quote.deleteBody')}</p>
      </ConfirmDialog>
    </div>
  )
}

/** Net and VAT under the total, and today's total where prices have moved since. */
function TotalDetails({ quote }: { quote: Quote }) {
  const { t } = useTranslation()
  const { money, percent } = useLocaleFormat()
  return (
    <>
      <p className="numeric mt-1 text-right text-xs text-ink-soft">
        {[
          t('quote.net', { amount: money(quote.total_net) }),
          ...quote.vat_groups.map((group) =>
            t('quote.vatGroup', { rate: percent(group.vat_percent), amount: money(group.vat) }),
          ),
        ].join(' · ')}
      </p>
      {quote.prices_changed ? (
        <p className="numeric mt-1 text-right text-xs font-semibold text-rust">
          {t('quote.todayTotal', { amount: money(quote.today_total_gross) })}
        </p>
      ) : null}
    </>
  )
}

interface QuoteLineCardProps {
  line: EditableLine
  /** The server's price for it; absent while it is being asked. */
  priced: QuoteLine | undefined
  error: string | undefined
  canLookUp: boolean
  onChange: (patch: Partial<EditableLine>) => void
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
      {priced?.price_changed ? (
        <p className="numeric mt-1 text-xs font-semibold text-rust">
          {priced.today_line_gross
            ? t('quote.priceChanged', {
                today: money(priced.today_line_gross),
                quoted: money(priced.line_gross),
              })
            : t('quote.priceGone')}
        </p>
      ) : null}
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
