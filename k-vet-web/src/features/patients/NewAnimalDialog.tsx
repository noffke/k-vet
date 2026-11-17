import { useQueryClient } from '@tanstack/react-query'
import { type FormEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createPatient, getListPatientsQueryKey, patchPatient } from '@/api/generated/endpoints'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { SelectField, TextField } from '@/components/ui/field'

const SEXES = ['female', 'male', 'unknown'] as const

interface NewAnimalDialogProps {
  open: boolean
  /** What was typed into the animal search, taken as the name. */
  term: string
  /** Whose animal it is: the visit's customer. */
  customerId: number
  onOpenChange: (open: boolean) => void
  /** Puts the new animal where it was asked for — on the visit — and opens it there. */
  onCreated: (patientId: number) => Promise<unknown>
}

/**
 * An animal the practice has not seen yet, made on the appointment it came to, for a customer
 * who is complete. Name, Tierart and Geschlecht are what make an animal complete, so they are
 * asked here and the animal can go onto the visit at once. Everything else is entered on the
 * animal's own page later. "Abbrechen" creates nothing.
 */
export function NewAnimalDialog({
  open,
  term,
  customerId,
  onOpenChange,
  onCreated,
}: NewAnimalDialogProps) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [name, setName] = useState('')
  const [species, setSpecies] = useState('')
  const [sex, setSex] = useState('')
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  // Made but not yet on the visit when the second step failed: a retry must not make it twice.
  const created = useRef<number | null>(null)

  // A fresh question each time it is asked, starting from what was typed.
  useEffect(() => {
    if (open) {
      setName(term.trim())
      setSpecies('')
      setSex('')
      setBusy(false)
      setFailed(false)
      created.current = null
    }
  }, [open, term])

  const ready = name.trim() !== '' && species.trim() !== '' && sex !== ''

  const create = async (event: FormEvent) => {
    event.preventDefault()
    if (!ready || busy) return
    setBusy(true)
    setFailed(false)
    try {
      if (created.current === null) {
        const patient = await createPatient({ customer_id: customerId })
        created.current = patient.id
      }
      const patientId = created.current
      await patchPatient(patientId, { name: name.trim(), species: species.trim(), sex })
      await client.invalidateQueries({ queryKey: getListPatientsQueryKey() })
      await onCreated(patientId)
      onOpenChange(false)
    } catch {
      setFailed(true)
    } finally {
      setBusy(false)
    }
  }

  const formId = 'new-animal'
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) onOpenChange(next)
      }}
      title={t('patients.newEntryTitle')}
      footer={
        <>
          <Button disabled={busy} onClick={() => onOpenChange(false)}>
            {t('action.cancel')}
          </Button>
          <Button variant="primary" type="submit" form={formId} disabled={busy || !ready}>
            {t('action.add')}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        onSubmit={(event) => void create(event)}
        className="grid gap-3 sm:grid-cols-3"
      >
        <TextField
          label={t('field.name')}
          value={name}
          autoFocus
          onChange={(event) => setName(event.target.value)}
          error={failed ? t('error.generic') : undefined}
        />
        {/* Cat and Dog are offered, anything else can be typed (FR-008). */}
        <TextField
          label={t('field.species')}
          list="new-animal-species"
          value={species}
          onChange={(event) => setSpecies(event.target.value)}
        />
        <datalist id="new-animal-species">
          <option value={t('species.cat')} />
          <option value={t('species.dog')} />
        </datalist>
        <SelectField
          label={t('field.sex')}
          value={sex}
          onChange={(event) => setSex(event.target.value)}
        >
          <option value="">—</option>
          {SEXES.map((option) => (
            <option key={option} value={option}>
              {t(`sex.${option}`)}
            </option>
          ))}
        </SelectField>
        <p className="text-sm text-ink-soft sm:col-span-3">{t('patients.newEntryHint')}</p>
      </form>
    </Dialog>
  )
}
