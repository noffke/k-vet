import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { type FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createManufacturer,
  createSupplier,
  getListManufacturersQueryKey,
  getListSuppliersQueryKey,
  patchManufacturer,
  patchSupplier,
} from '@/api/generated/endpoints'
import type { AddressBookEntry } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { TextField } from '@/components/ui/field'

/** The value of the "Neuer Hersteller …" / "Neuer Lieferant …" entry at the end of a select. */
export const NEW_ENTRY = 'new'

interface NewEntryDialogProps {
  kind: 'manufacturer' | 'supplier'
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Sets the new entry where it was asked for — the drug, the packaging — before leaving. */
  onCreated: (entry: AddressBookEntry) => Promise<unknown>
}

/**
 * A manufacturer or supplier made from the select that needs it, without leaving for
 * Stammdaten first. The name is asked for here, so the entry is complete from the start and a
 * stray click is only "Abbrechen". Once created it is set where it was asked for, and its own
 * page opens for the address; "Zurück" there leads back to the drug, which shows it chosen.
 */
export function NewEntryDialog({ kind, open, onOpenChange, onCreated }: NewEntryDialogProps) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const navigate = useNavigate()
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const isManufacturer = kind === 'manufacturer'

  const close = (next: boolean) => {
    if (!next) {
      setName('')
      setFailed(false)
    }
    onOpenChange(next)
  }

  const create = async (event: FormEvent) => {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || busy) return
    setBusy(true)
    setFailed(false)
    try {
      const entry = isManufacturer
        ? await patchManufacturer((await createManufacturer()).id, { name: trimmed })
        : await patchSupplier((await createSupplier()).id, { name: trimmed })
      await client.invalidateQueries({
        queryKey: isManufacturer ? getListManufacturersQueryKey() : getListSuppliersQueryKey(),
      })
      await onCreated(entry)
      close(false)
      await navigate({
        to: isManufacturer ? '/masterdata/manufacturers/$id' : '/masterdata/suppliers/$id',
        params: { id: String(entry.id) },
      })
    } catch {
      setFailed(true)
    } finally {
      setBusy(false)
    }
  }

  const formId = `new-${kind}`
  return (
    <Dialog
      open={open}
      onOpenChange={close}
      title={isManufacturer ? t('masterData.newManufacturer') : t('masterData.newSupplier')}
      footer={
        <>
          <Button onClick={() => close(false)}>{t('action.cancel')}</Button>
          <Button variant="primary" type="submit" form={formId} disabled={busy || !name.trim()}>
            {t('masterData.createAndOpen')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={(event) => void create(event)} className="flex flex-col gap-3">
        <TextField
          label={t('field.name')}
          value={name}
          autoFocus
          onChange={(event) => setName(event.target.value)}
          error={failed ? t('error.generic') : undefined}
        />
        <p className="text-sm text-ink-soft">{t('masterData.newEntryHint')}</p>
      </form>
    </Dialog>
  )
}
