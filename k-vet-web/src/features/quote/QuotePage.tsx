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
import { PositionList, PositionRow } from '@/components/PositionRow'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { TextAreaField, TextField } from '@/components/ui/field'
import { ValueCell } from '@/components/ValueCell'
import { useKmToast } from '@/features/treatments/kmToast'
import type { SaveState } from '@/lib/autosave'
import { useOperatorConfig } from '@/lib/config'
import { dispenseBreakdown, dispenseRuleText, refusalText } from '@/lib/dispense'
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
  const { money, quantity: formatQuantity } = useLocaleFormat()
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
      if (target === undefined && !hasContent(current)) return
      // Taken before anything is awaited: what is edited while this save is under way — a
      // position picked while the record is still being created — is the next save's, not
      // marked as written by this one.
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
        if (target === undefined) {
          const created = await createQuote({ customer_id: null })
          target = created.id
          quoteId.current = target
          setLoadedFor(target)
          void navigate({ to: '/quote', search: { id: target }, replace: true })
        }
        const saved = await patchQuote(target, data)
        client.setQueryData(getGetQuoteQueryKey(target), saved)
        setSaveError(null)
        setSaveState('saved')
      } catch (error) {
        // Not written, so still to be written by the next save; and a failed create no longer
        // leaves the chain rejected, which stopped every later save.
        dirty.current.fields ||= fields
        dirty.current.lines ||= lines
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

  // The line just picked keeps its values open, its quantity focused; Enter goes back to the
  // search. Its key is made in the draft's update, so it is found by what was there before.
  const [newestKey, setNewestKey] = useState<string | null>(null)
  const keysBeforePick = useRef<Set<string> | null>(null)
  const search = useRef<HTMLInputElement | null>(null)
  useEffect(() => {
    const before = keysBeforePick.current
    if (!before) return
    const added = draft.lines.find((line) => !before.has(line.key))
    if (!added) return
    keysBeforePick.current = null
    setNewestKey(added.key)
  }, [draft.lines])

  const pick = (item: PickerItem) => {
    keysBeforePick.current = new Set(draftRef.current.lines.map((line) => line.key))
    edit(
      (current) =>
        addPositions(current, [
          {
            kind: item.kind,
            id: item.id,
            name: item.name,
            // A drug starts at what it allows — one unit, else one step.
            quantity: item.kind === 'drug_packaging' ? item.rule.preset : undefined,
          },
        ]),
      'lines',
    )
  }

  const pickGroup = async (template: Template) => {
    setNewestKey(null)
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
    <div className="mx-auto max-w-5xl">
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
        {draft.lines.length > 0 ? (
          <PositionList label={t('quote.positions')}>
            {draft.lines.map((line, index) => (
              <QuoteLineCard
                key={line.key}
                newest={line.key === newestKey}
                onReturnToSearch={() => search.current?.focus()}
                line={line}
                priced={pricedLine(index, line)}
                error={
                  refusal?.index === index
                    ? refusalText(
                        refusal.message,
                        pricedLine(index, line)?.today_rule,
                        pricedLine(index, line)?.unit,
                        t,
                        formatQuantity,
                      )
                    : undefined
                }
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
          </PositionList>
        ) : (
          <p className="mt-2 rounded-card border border-dashed border-line px-3 py-6 text-center text-sm text-ink-faint">
            {t('quote.empty')}
          </p>
        )}
        {/* Under the list: a picked position appears right above the search it came from. */}
        <div className="mt-3">
          <ItemPicker
            onPick={pick}
            inputRef={search}
            onPickTemplate={(template) => void pickGroup(template)}
          />
        </div>
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
  /** The line just picked: marked, and a drug's quantity open and focused. */
  newest: boolean
  /** Enter in the newest line's value: back to the search. */
  onReturnToSearch: () => void
  onChange: (patch: Partial<EditableLine>) => void
  /** Asks for the kilometres again. */
  onLookUp: () => void
  onRemove: () => void
}

type QuoteCell = 'quantity' | 'factor' | 'km'

/** One position of the quote: one line — what it is, how much, what it costs — the rest behind. */
function QuoteLineCard({
  line,
  priced,
  error,
  canLookUp,
  newest,
  onReturnToSearch,
  onChange,
  onLookUp,
  onRemove,
}: QuoteLineCardProps) {
  const { t } = useTranslation()
  const { money, percent, unitPrice, quantity } = useLocaleFormat()
  const [editing, setEditing] = useState<QuoteCell | null>(null)
  const Icon = line.kind === 'drug_packaging' ? Pill : Stethoscope
  const rule = priced?.today_rule ?? priced?.dispense?.rule
  const travel = priced?.travel_expenses ?? false
  // The drug just picked opens its quantity and takes the focus; a service mostly stays 1 × at
  // 100 % (2026-11-22), so the search keeps it for the next position.
  const typed = newest && line.kind !== 'service'
  const isOpen = (cell: QuoteCell) => typed || editing === cell
  const closeCell = (cell: QuoteCell) => () =>
    setEditing((current) => (current === cell ? null : current))
  const breakdown = priced?.dispense
    ? dispenseBreakdown(priced.dispense, priced.unit, t, { quantity, money, unitPrice })
    : null
  const factor = line.factor ?? priced?.factor ?? null
  const typing = isOpen('quantity') || isOpen('factor') || isOpen('km')
  const meta = priced
    ? `${priced.got_number ? `GOT ${priced.got_number} · ` : ''}${percent(priced.vat_percent)} ${t('field.vat')}`
    : null

  return (
    <PositionRow
      icon={
        <Icon
          className={cn(
            'size-4 shrink-0',
            line.kind === 'drug_packaging' ? 'text-rust' : 'text-ink-soft',
          )}
          aria-hidden
        />
      }
      name={priced?.name ?? line.name}
      newest={newest}
      quantity={
        travel ? (
          <ValueCell
            label={t('field.km')}
            display={line.km != null ? `${quantity(line.km)} km` : t('treatments.kmMissingShort')}
            missing={line.km == null}
            value={line.km}
            unit="km"
            open={isOpen('km')}
            onOpen={() => setEditing('km')}
            onClose={closeCell('km')}
            autoFocus={typed}
            onEnter={typed ? onReturnToSearch : undefined}
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
        ) : (
          <ValueCell
            label={t('field.quantity')}
            display={`${quantity(line.quantity)} ${line.kind === 'service' ? '×' : (priced?.unit ?? '')}`.trim()}
            value={line.quantity}
            unit={priced?.unit ?? undefined}
            open={isOpen('quantity')}
            onOpen={() => setEditing('quantity')}
            onClose={closeCell('quantity')}
            autoFocus={typed}
            onEnter={typed ? onReturnToSearch : undefined}
            onChange={(value) => value && onChange({ quantity: value })}
          />
        )
      }
      factor={
        line.kind === 'service' ? (
          <ValueCell
            label={t('field.factor')}
            display={percent(factor)}
            value={factor}
            decimals={3}
            unit="%"
            open={isOpen('factor')}
            onOpen={() => setEditing('factor')}
            onClose={closeCell('factor')}
            onEnter={typed ? onReturnToSearch : undefined}
            onChange={(value) => onChange({ factor: value })}
          />
        ) : null
      }
      amount={priced ? money(priced.line_gross) : '—'}
      // While a value is typed: the steps, how it bills, and GOT and VAT — said here once.
      hint={
        typing && priced ? (
          <span className="numeric">
            {[
              !travel && rule ? dispenseRuleText(rule, priced.unit, t, quantity) : null,
              breakdown,
              meta,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        ) : undefined
      }
      notice={
        priced?.price_changed || error || (travel && line.km == null) ? (
          <>
            {travel && line.km == null ? (
              <span className="font-medium text-danger">{t('quote.kmMissing')}</span>
            ) : null}
            {priced?.price_changed ? (
              <span className="numeric font-semibold text-rust">
                {priced.today_line_gross
                  ? t('quote.priceChanged', {
                      today: money(priced.today_line_gross),
                      quoted: money(priced.line_gross),
                    })
                  : t('quote.priceGone')}
              </span>
            ) : null}
            {error ? (
              <span className="text-danger" role="alert">
                {error}
              </span>
            ) : null}
          </>
        ) : undefined
      }
      removeLabel={t('quote.removeLine')}
      onRemove={onRemove}
      details={
        <div className="flex flex-col gap-3">
          {travel ? (
            <div className="grid gap-3 sm:grid-cols-3">
              <NumberInput
                label={t('field.quantity')}
                value={line.quantity}
                onChange={(value) => value && onChange({ quantity: value })}
              />
              <NumberInput
                label={t('field.kmMultiplier')}
                value={line.kmMultiplier}
                decimals={3}
                hint={t('treatments.kmHint')}
                onChange={(value) => onChange({ kmMultiplier: value })}
              />
            </div>
          ) : null}
          {/* GOT, VAT and the breakdown — unless the subline is saying them right now. */}
          {typing ? null : (
            <div className="text-xs text-ink-faint">
              {meta ? <p>{meta}</p> : null}
              {breakdown ? <p className="numeric text-ink-soft">{breakdown}</p> : null}
            </div>
          )}
        </div>
      }
    />
  )
}
