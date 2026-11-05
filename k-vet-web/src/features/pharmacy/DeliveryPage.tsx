import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { CircleHelp, PackageCheck, PackagePlus, Plus, Undo2, X } from 'lucide-react'
import { useEffect, useReducer, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  addDrugCode,
  createDrug,
  createStockIntake,
  deleteLot,
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  getListLotsQueryKey,
  getListPackagingsQueryKey,
  patchDrug,
  readScan,
} from '@/api/generated/endpoints'
import { BackLink } from '@/components/BackLink'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ItemPicker } from '@/components/ItemPicker'
import { PageHeader } from '@/components/PageHeader'
import { ScanField } from '@/components/ScanField'
import { Button } from '@/components/ui/button'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'
import { type BookedPack, deliveryReducer, persist } from './delivery'
import { IntakeFields } from './IntakeFields'
import { canBook, intakeBody } from './intake'

/**
 * A wholesaler's box, booked pack by pack with a barcode scanner: scan a pack, check what is
 * asked, Enter, next pack. One page, no steps: the scan field keeps the focus, the pack in hand
 * sits below it, and what is booked so far below that.
 *
 * Each scan is read by the server (`readScan`); what it means for the delivery is
 * `deliveryReducer`'s call. A code no drug has can be added to a drug, or a new drug made for it
 * — whose page "Zurück" leaves for this one again, where the delivery waits in session storage.
 */
export function DeliveryPage() {
  const { t } = useTranslation()
  const { date, quantity } = useLocaleFormat()
  const client = useQueryClient()
  const navigate = useNavigate()
  const { vat_rates } = useOperatorConfig()
  const [state, dispatch] = useReducer(deliveryReducer, undefined, persist.load)
  const [undoing, setUndoing] = useState<BookedPack | null>(null)
  const scanField = useRef<HTMLInputElement>(null)
  const focusScan = () => scanField.current?.focus()

  useEffect(() => persist.save(state), [state])

  const failed = (error: unknown) =>
    dispatch({
      type: 'failed',
      notice: {
        key:
          error instanceof ApiError && error.status === 409
            ? 'pharmacy.codeTaken'
            : 'error.generic',
      },
    })

  const scan = async (raw: string) => {
    try {
      dispatch({ type: 'scanned', raw, result: await readScan({ raw }) })
    } catch (error) {
      failed(error)
    }
  }

  // Back from a new drug's page: the waiting code names that drug now.
  const rescanned = useRef(false)
  useEffect(() => {
    if (rescanned.current || !state.unknown) return
    rescanned.current = true
    void scan(state.unknown.raw)
  })

  const refresh = async (drugId: number) => {
    await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
    await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
    await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drugId) })
    await client.invalidateQueries({ queryKey: getListPackagingsQueryKey(drugId) })
  }

  const book = async () => {
    const current = state.current
    if (!current || !canBook(current.intake)) return
    try {
      const lot = await createStockIntake(
        current.original.id,
        intakeBody(current.intake, current.original),
      )
      dispatch({ type: 'booked', lotId: lot.id })
      await refresh(current.drug.id)
    } catch (error) {
      failed(error)
    }
    focusScan()
  }

  const assign = async (drugId: number) => {
    const unknown = state.unknown
    if (!unknown) return
    try {
      await addDrugCode(drugId, { code: unknown.raw })
      await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drugId) })
      await scan(unknown.raw)
    } catch (error) {
      failed(error)
    }
    focusScan()
  }

  const newDrug = async () => {
    const unknown = state.unknown
    if (!unknown) return
    try {
      const drug = await createDrug()
      // Almost every drug is billed at the standard rate, as "Neues Medikament" does it.
      const standardRate = vat_rates[0]
      if (standardRate) await patchDrug(drug.id, { vat_percent: standardRate })
      await addDrugCode(drug.id, { code: unknown.raw })
      await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
      // The code waits here; on the way back it names the new drug.
      await navigate({ to: '/pharmacy/$id', params: { id: String(drug.id) } })
    } catch (error) {
      failed(error)
    }
  }

  const addNewCode = async () => {
    const current = state.current
    if (!current?.newCode) return
    try {
      client.setQueryData(
        getGetDrugQueryKey(current.drug.id),
        await addDrugCode(current.drug.id, { code: current.newCode.raw }),
      )
      dispatch({ type: 'codeAdded' })
    } catch (error) {
      failed(error)
    }
    focusScan()
  }

  const undo = async (pack: BookedPack) => {
    try {
      await deleteLot(pack.lotId)
      dispatch({ type: 'undone', lotId: pack.lotId })
      await client.invalidateQueries({ queryKey: getListLotsQueryKey() })
      await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
    } catch (error) {
      dispatch({
        type: 'failed',
        notice: {
          key:
            error instanceof ApiError && error.status === 409
              ? 'delivery.undoRefused'
              : 'error.generic',
        },
      })
    }
    focusScan()
  }

  const current = state.current
  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/pharmacy" label={t('pharmacy.title')} />}
        eyebrow={t('pharmacy.title')}
        title={t('delivery.title')}
        actions={
          state.booked.length > 0 ? (
            <Button onClick={() => dispatch({ type: 'finished' })}>
              <PackageCheck className="size-4" />
              {t('delivery.finish')}
            </Button>
          ) : null
        }
      />

      <section className="mt-5 flex flex-col gap-2 rounded-card border border-line bg-surface p-4">
        <ScanField
          ref={scanField}
          label={t('delivery.scan')}
          placeholder={t('delivery.scanPlaceholder')}
          autoFocus
          onScan={(raw) => void scan(raw)}
          onEnterEmpty={() => void book()}
        />
        <p className="text-xs text-ink-faint">{t('delivery.hint')}</p>
        {state.notice ? (
          <p
            role="alert"
            className="rounded-card border border-danger/30 bg-danger/5 px-3 py-2 text-sm text-danger"
          >
            {t(state.notice.key, state.notice.values)}
          </p>
        ) : null}
      </section>

      {state.unknown ? (
        <section className="mt-4 flex flex-col gap-3 rounded-card border border-rust/40 bg-surface p-4">
          <h2 className="flex items-center gap-2 text-base">
            <CircleHelp className="size-4 text-rust" aria-hidden />
            {t('delivery.unknown', { code: state.unknown.display })}
          </h2>
          <p className="text-sm text-ink-soft">{t('delivery.unknownHint')}</p>
          <ItemPicker
            drugsOnly
            placeholder={t('delivery.assignPlaceholder')}
            onPick={(item) => {
              if (item.kind === 'drug_packaging') void assign(item.drug_id)
            }}
          />
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void newDrug()}>
              <Plus className="size-4" />
              {t('delivery.newDrug')}
            </Button>
            <Button variant="ghost" onClick={() => dispatch({ type: 'discarded' })}>
              <X className="size-4" />
              {t('delivery.discard')}
            </Button>
          </div>
        </section>
      ) : null}

      {current ? (
        <section className="mt-4 flex flex-col gap-4 rounded-card border border-line bg-surface p-4">
          <div>
            <p className="eyebrow">{t('delivery.current')}</p>
            <h2 className="text-lg">{current.drug.name}</h2>
            <p className="numeric text-sm text-ink-soft">
              {t('delivery.originalPack', {
                quantity: quantity(current.original.quantity ?? ''),
                unit: current.original.unit ?? '',
              })}
            </p>
          </div>
          {current.newCode ? (
            <div className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
              {t('delivery.newCodeOffer', {
                code: current.newCode.display,
                name: current.drug.name ?? '',
              })}
              <Button size="small" onClick={() => void addNewCode()}>
                {t('delivery.addCode')}
              </Button>
            </div>
          ) : null}
          <IntakeFields
            drug={current.drug}
            original={current.original}
            value={current.intake}
            onChange={(intake) => dispatch({ type: 'edited', intake })}
          />
          <div className="flex flex-wrap items-center justify-end gap-2">
            <Button variant="ghost" onClick={() => dispatch({ type: 'discarded' })}>
              <X className="size-4" />
              {t('delivery.discard')}
            </Button>
            <Button
              variant="primary"
              disabled={!canBook(current.intake)}
              onClick={() => void book()}
            >
              <PackagePlus className="size-4" />
              {t('delivery.book')}
            </Button>
          </div>
        </section>
      ) : null}

      <section className="mt-6">
        <h2 className="text-base">
          {t('delivery.booked')}{' '}
          <span className="numeric text-ink-faint">{state.booked.length}</span>
        </h2>
        {state.booked.length === 0 ? (
          <p className="mt-2 text-sm text-ink-faint">{t('delivery.nothingBooked')}</p>
        ) : (
          <ul className="mt-2 flex flex-col gap-2">
            {state.booked.map((pack) => (
              <li
                key={pack.lotId}
                className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-line bg-surface px-3 py-2"
              >
                <span className="flex flex-col">
                  <span className="text-ink">{pack.drugName}</span>
                  <span className="numeric text-xs text-ink-soft">
                    {[
                      t('delivery.packages', { count: pack.packages }),
                      pack.batch,
                      pack.expiry ? date(pack.expiry) : null,
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </span>
                </span>
                <Button size="small" variant="ghost" onClick={() => setUndoing(pack)}>
                  <Undo2 className="size-4" />
                  {t('delivery.undo')}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <ConfirmDialog
        open={undoing !== null}
        onOpenChange={(open) => {
          if (!open) setUndoing(null)
        }}
        title={t('delivery.undoTitle')}
        confirmLabel={t('delivery.undo')}
        onConfirm={() => {
          if (undoing) void undo(undoing)
        }}
      >
        <p>
          {t('delivery.undoBody', {
            count: undoing?.packages ?? 0,
            name: undoing?.drugName ?? '',
          })}
        </p>
      </ConfirmDialog>
    </div>
  )
}
