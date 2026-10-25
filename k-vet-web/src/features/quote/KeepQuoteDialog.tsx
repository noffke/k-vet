import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useGetCustomer } from '@/api/generated/endpoints'
import type { Salutation } from '@/api/generated/model'
import { CustomerPicker } from '@/components/CustomerPicker'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { SelectField, TextField } from '@/components/ui/field'
import { addressLine } from '@/lib/maps'
import type { QuoteAddress } from './quoteState'

const SALUTATIONS: Salutation[] = ['frau', 'herr', 'familie']

/** Whom a kept quote belongs to, and what it is called. */
export interface KeepChoice {
  name: string
  customer:
    | { kind: 'current' }
    | { kind: 'existing'; id: number }
    | { kind: 'new'; salutation: Salutation; firstName: string; lastName: string }
}

interface KeepQuoteDialogProps {
  open: boolean
  name: string
  /** The quote's customer, if it has one already. */
  customerId: number | null
  /** A new caller's typed address, taken over by a new customer. */
  address: QuoteAddress
  onKeep: (choice: KeepChoice) => Promise<void>
  onDiscard: () => Promise<void>
  onCancel: () => void
}

/**
 * Leaving an open quote: keep it — with a customer, a known one or one made from the caller's
 * name and the typed address — or discard it, or stay. A quote is kept with a customer so it
 * can be found on their page and taken into their visit.
 */
export function KeepQuoteDialog({
  open,
  name,
  customerId,
  address,
  onKeep,
  onDiscard,
  onCancel,
}: KeepQuoteDialogProps) {
  const { t } = useTranslation()
  const [title, setTitle] = useState(name)
  const [mode, setMode] = useState<'existing' | 'new'>('existing')
  const [picked, setPicked] = useState<number | null>(null)
  const [salutation, setSalutation] = useState<Salutation | ''>('')
  const [firstName, setFirstName] = useState('')
  const [lastName, setLastName] = useState('')
  const [busy, setBusy] = useState(false)
  const pickedCustomer = useGetCustomer(picked ?? 0, { query: { enabled: picked !== null } })

  // A fresh question each time it is asked.
  useEffect(() => {
    if (open) {
      setTitle(name)
      setBusy(false)
    }
  }, [open, name])

  const typedAddress = addressLine(address)
  const ready =
    customerId !== null ||
    (mode === 'existing' ? picked !== null : salutation !== '' && lastName.trim() !== '')

  const run = async (action: () => Promise<void>) => {
    setBusy(true)
    try {
      await action()
    } finally {
      setBusy(false)
    }
  }

  const keep = () =>
    run(() =>
      onKeep({
        name: title,
        customer:
          customerId !== null
            ? { kind: 'current' }
            : mode === 'existing' && picked !== null
              ? { kind: 'existing', id: picked }
              : {
                  kind: 'new',
                  salutation: salutation as Salutation,
                  firstName,
                  lastName,
                },
      }),
    )

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onCancel()
      }}
      title={t('quote.keepTitle')}
      footer={
        <>
          <Button disabled={busy} onClick={onCancel}>
            {t('action.cancel')}
          </Button>
          <Button variant="danger" disabled={busy} onClick={() => void run(onDiscard)}>
            {t('quote.discard')}
          </Button>
          <Button variant="primary" disabled={busy || !ready} onClick={() => void keep()}>
            {t('quote.keep')}
          </Button>
        </>
      }
    >
      <p className="text-sm text-ink-soft">{t('quote.keepBody')}</p>
      <div className="mt-3 flex flex-col gap-3">
        <TextField
          label={t('quote.name')}
          value={title}
          placeholder={t('quote.namePlaceholder')}
          onChange={(event) => setTitle(event.target.value)}
        />
        {customerId !== null ? null : (
          <fieldset className="border-0 p-0">
            <legend className="text-xs font-semibold text-ink-soft">{t('quote.forWhom')}</legend>
            <div className="mt-1 flex gap-4">
              {(['existing', 'new'] as const).map((option) => (
                <label key={option} className="flex min-h-11 items-center gap-2 text-sm sm:min-h-9">
                  <input
                    type="radio"
                    name="keep-customer"
                    className="size-4 accent-ink"
                    checked={mode === option}
                    onChange={() => setMode(option)}
                  />
                  {t(option === 'existing' ? 'quote.existingCustomer' : 'quote.newCustomer')}
                </label>
              ))}
            </div>
            {mode === 'existing' ? (
              <div className="mt-2">
                <CustomerPicker
                  customerId={picked}
                  customerName={
                    [pickedCustomer.data?.first_name, pickedCustomer.data?.last_name]
                      .filter(Boolean)
                      .join(' ') || null
                  }
                  onPick={setPicked}
                  onClear={() => setPicked(null)}
                />
              </div>
            ) : (
              <div className="mt-2 grid gap-3 sm:grid-cols-3">
                <SelectField
                  label={t('field.salutation')}
                  value={salutation}
                  onChange={(event) => setSalutation(event.target.value as Salutation | '')}
                >
                  <option value="">—</option>
                  {SALUTATIONS.map((option) => (
                    <option key={option} value={option}>
                      {t(`salutation.${option}`)}
                    </option>
                  ))}
                </SelectField>
                <TextField
                  label={t('field.firstName')}
                  value={firstName}
                  onChange={(event) => setFirstName(event.target.value)}
                />
                <TextField
                  label={t('field.lastName')}
                  value={lastName}
                  onChange={(event) => setLastName(event.target.value)}
                />
                <p className="text-xs text-ink-faint sm:col-span-3">
                  {typedAddress
                    ? t('quote.newCustomerAddress', { address: typedAddress })
                    : t('quote.newCustomerNoAddress')}
                </p>
              </div>
            )}
          </fieldset>
        )}
      </div>
    </Dialog>
  )
}
