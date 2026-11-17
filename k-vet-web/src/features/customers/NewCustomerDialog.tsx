import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { type FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createCustomer, getListCustomersQueryKey, patchCustomer } from '@/api/generated/endpoints'
import type { Salutation } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { SelectField, TextField } from '@/components/ui/field'

const SALUTATIONS: Salutation[] = ['frau', 'herr', 'familie']

interface NewCustomerDialogProps {
  open: boolean
  /** What was typed into the search, taken as the name: "Erika Muster" or just "Muster". */
  term: string
  onOpenChange: (open: boolean) => void
  /** Sets the new customer where it was asked for — the appointment — before leaving. */
  onCreated: (customerId: number) => Promise<unknown>
}

/** "Erika Muster" → first name "Erika", last name "Muster"; a single word is the last name. */
export function splitName(term: string): { firstName: string; lastName: string } {
  const words = term.trim().split(/\s+/).filter(Boolean)
  const lastName = words.pop() ?? ''
  return { firstName: words.join(' '), lastName }
}

/**
 * A caller who is not a customer yet, made from the appointment instead of leaving for
 * Kunden first. Anrede and name are asked here; once created the customer is set on the
 * appointment and their own page opens for the address and the animals. "Zurück" there leads
 * back to the appointment, which shows them chosen. Until the address is entered the customer
 * stays incomplete, as any new one does; "Abbrechen" creates nothing.
 */
export function NewCustomerDialog({ open, term, onOpenChange, onCreated }: NewCustomerDialogProps) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const navigate = useNavigate()
  const [salutation, setSalutation] = useState<Salutation | ''>('')
  const [firstName, setFirstName] = useState('')
  const [lastName, setLastName] = useState('')
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)

  // A fresh question each time it is asked, starting from what was typed.
  useEffect(() => {
    if (open) {
      const name = splitName(term)
      setSalutation('')
      setFirstName(name.firstName)
      setLastName(name.lastName)
      setBusy(false)
      setFailed(false)
    }
  }, [open, term])

  const ready = salutation !== '' && lastName.trim() !== ''

  const create = async (event: FormEvent) => {
    event.preventDefault()
    if (!ready || busy) return
    setBusy(true)
    setFailed(false)
    try {
      const created = await createCustomer()
      await patchCustomer(created.id, {
        salutation: salutation as Salutation,
        first_name: firstName.trim() || null,
        last_name: lastName.trim(),
      })
      await client.invalidateQueries({ queryKey: getListCustomersQueryKey() })
      await onCreated(created.id)
      onOpenChange(false)
      await navigate({ to: '/customers/$id', params: { id: String(created.id) } })
    } catch {
      setFailed(true)
    } finally {
      setBusy(false)
    }
  }

  const formId = 'new-customer'
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) onOpenChange(next)
      }}
      title={t('customers.new')}
      footer={
        <>
          <Button disabled={busy} onClick={() => onOpenChange(false)}>
            {t('action.cancel')}
          </Button>
          <Button variant="primary" type="submit" form={formId} disabled={busy || !ready}>
            {t('action.createAndOpen')}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        onSubmit={(event) => void create(event)}
        className="grid gap-3 sm:grid-cols-3"
      >
        <SelectField
          label={t('field.salutation')}
          value={salutation}
          autoFocus
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
          error={failed ? t('error.generic') : undefined}
        />
        <p className="text-sm text-ink-soft sm:col-span-3">{t('customers.newEntryHint')}</p>
      </form>
    </Dialog>
  )
}
