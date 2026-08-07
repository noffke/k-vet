import { useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from '@tanstack/react-router'
import { Archive, ArchiveRestore, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getGetCustomerQueryKey,
  getListCustomersQueryKey,
  getListPatientsQueryKey,
  useAddCustomerEmail,
  useArchiveCustomer,
  useCreatePatient,
  useDeleteCustomerEmail,
  useGetCustomer,
  useListPatients,
  usePatchCustomer,
  useUnarchiveCustomer,
} from '@/api/generated/endpoints'
import type { Customer, EmailType, Salutation } from '@/api/generated/model'
import { BackLink } from '@/components/BackLink'
import { PageHeader } from '@/components/PageHeader'
import { ArchivedBadge, IncompleteBadge } from '@/components/RecordBadges'
import { SaveIndicator } from '@/components/SaveIndicator'
import { Button } from '@/components/ui/button'
import { SelectField, TextAreaField, TextField } from '@/components/ui/field'
import { WarningBanner } from '@/components/WarningBanner'
import { useAutoSave } from '@/lib/autosave'
import { useOperatorConfig } from '@/lib/config'

const SALUTATIONS: Salutation[] = ['frau', 'herr', 'familie']
const EMAIL_TYPES: EmailType[] = ['private', 'work', 'other']

/**
 * One customer. Every field auto-saves on the keystroke after the last; the record stays
 * "incomplete" until salutation, last name and home address are known.
 */
export function CustomerDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams({ from: '/app/customers/$id' })
  const customerId = Number(id)
  const client = useQueryClient()
  const navigate = useNavigate()

  const { default_country } = useOperatorConfig()
  const customer = useGetCustomer(customerId)
  const patients = useListPatients({ customer_id: customerId })
  const patchCustomer = usePatchCustomer()
  const addEmail = useAddCustomerEmail()
  const deleteEmail = useDeleteCustomerEmail()
  const archive = useArchiveCustomer()
  const unarchive = useUnarchiveCustomer()
  const createPatient = useCreatePatient()
  const [newEmail, setNewEmail] = useState('')
  const [newEmailType, setNewEmailType] = useState<EmailType>('private')
  const [showInvoiceAddress, setShowInvoiceAddress] = useState(false)

  const store = (updated: Customer) => {
    client.setQueryData(getGetCustomerQueryKey(customerId), updated)
    void client.invalidateQueries({ queryKey: getListCustomersQueryKey() })
  }

  const autoSave = useAutoSave<Customer>({
    save: (patch) =>
      patchCustomer.mutateAsync({
        id: customerId,
        data: patch as Parameters<typeof patchCustomer.mutateAsync>[0]['data'],
      }),
    onSaved: store,
  })

  if (customer.isPending) return <p className="text-sm text-ink-faint">{t('list.loading')}</p>
  if (!customer.data) return <p className="text-sm text-danger">{t('error.notFound')}</p>
  const record = customer.data
  const invoiceAddressVisible = showInvoiceAddress || record.has_invoice_address

  /** Text field bound to auto-save; the field error comes from the server. */
  const field = (
    key: keyof Customer & string,
    label: string,
    options: { className?: string; fallback?: string } = {},
  ) => (
    <TextField
      label={label}
      defaultValue={(record[key] as string | null) ?? options.fallback ?? ''}
      error={autoSave.fieldErrors[key] ? t(autoSave.fieldErrors[key] ?? '') : undefined}
      wrapperClassName={options.className}
      onChange={(event) => autoSave.set({ [key]: event.target.value || null } as Partial<Customer>)}
      onBlur={() => void autoSave.flush()}
    />
  )

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        back={<BackLink to="/customers" label={t('customers.title')} />}
        title={
          [record.first_name, record.last_name].filter(Boolean).join(' ') || t('customers.new')
        }
        actions={
          <>
            <SaveIndicator state={autoSave.state} error={autoSave.error} />
            {record.archived ? (
              <Button onClick={() => unarchive.mutate({ id: customerId }, { onSuccess: store })}>
                <ArchiveRestore className="size-4" />
                <span className="sr-only sm:not-sr-only">{t('record.unarchive')}</span>
              </Button>
            ) : (
              <Button onClick={() => archive.mutate({ id: customerId }, { onSuccess: store })}>
                <Archive className="size-4" />
                <span className="sr-only sm:not-sr-only">{t('record.archive')}</span>
              </Button>
            )}
          </>
        }
      >
        <span className="flex gap-1">
          <IncompleteBadge missing={record.missing_fields} />
          <ArchivedBadge archived={record.archived} />
        </span>
      </PageHeader>

      <WarningBanner className="mt-4" remark={record.warning_remark ?? ''} />

      <section className="mt-5 grid gap-4 rounded-card border border-line bg-surface p-4 sm:grid-cols-2">
        <SelectField
          label={t('field.salutation')}
          defaultValue={record.salutation ?? ''}
          onChange={(event) =>
            autoSave.set({ salutation: (event.target.value || null) as Salutation })
          }
        >
          <option value="">—</option>
          {SALUTATIONS.map((salutation) => (
            <option key={salutation} value={salutation}>
              {t(`salutation.${salutation}`)}
            </option>
          ))}
        </SelectField>
        {field('first_name', t('field.firstName'))}
        {field('last_name', t('field.lastName'), { className: 'sm:col-span-2' })}

        <fieldset className="border-0 p-0 sm:col-span-2">
          <legend className="eyebrow">{t('field.secondName')}</legend>
          <div className="mt-2 grid gap-4 sm:grid-cols-3">
            <SelectField
              label={t('field.salutation')}
              defaultValue={record.second_salutation ?? ''}
              onChange={(event) =>
                autoSave.set({
                  second_salutation: (event.target.value || null) as Salutation,
                })
              }
            >
              <option value="">—</option>
              {SALUTATIONS.map((salutation) => (
                <option key={salutation} value={salutation}>
                  {t(`salutation.${salutation}`)}
                </option>
              ))}
            </SelectField>
            {field('second_first_name', t('field.firstName'))}
            {field('second_last_name', t('field.lastName'))}
          </div>
        </fieldset>

        <fieldset className="border-0 p-0 sm:col-span-2">
          <legend className="eyebrow">{t('field.homeAddress')}</legend>
          <div className="mt-2 grid gap-4 sm:grid-cols-4">
            {field('company', t('field.company'), { className: 'sm:col-span-4' })}
            {field('home_addon', t('field.addon'), { className: 'sm:col-span-4' })}
            {field('home_street', t('field.street'), { className: 'sm:col-span-4' })}
            {field('home_zip', t('field.zip'))}
            {field('home_city', t('field.city'), { className: 'sm:col-span-2' })}
            {field('home_country', t('field.country'), { fallback: default_country })}
          </div>
        </fieldset>

        <TextField
          label={t('field.phone')}
          defaultValue={record.phone_display ?? ''}
          error={autoSave.fieldErrors.phone ? t(autoSave.fieldErrors.phone ?? '') : undefined}
          inputMode="tel"
          onChange={(event) => autoSave.set({ phone: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />
        <TextAreaField
          label={t('field.warningRemark')}
          defaultValue={record.warning_remark ?? ''}
          wrapperClassName="sm:col-span-2"
          onChange={(event) => autoSave.set({ warning_remark: event.target.value || null })}
          onBlur={() => void autoSave.flush()}
        />

        {invoiceAddressVisible ? (
          <fieldset className="border-0 p-0 sm:col-span-2">
            <legend className="eyebrow">{t('field.invoiceAddress')}</legend>
            <div className="mt-2 grid gap-4 sm:grid-cols-4">
              <SelectField
                label={t('field.salutation')}
                defaultValue={record.invoice_salutation ?? ''}
                onChange={(event) =>
                  autoSave.set({
                    invoice_salutation: (event.target.value || null) as Salutation,
                  })
                }
              >
                <option value="">—</option>
                {SALUTATIONS.map((salutation) => (
                  <option key={salutation} value={salutation}>
                    {t(`salutation.${salutation}`)}
                  </option>
                ))}
              </SelectField>
              {field('invoice_company', t('field.company'), { className: 'sm:col-span-4' })}
              {field('invoice_first_name', t('field.firstName'))}
              {field('invoice_last_name', t('field.lastName'), { className: 'sm:col-span-2' })}
              {field('invoice_addon', t('field.addon'), { className: 'sm:col-span-4' })}
              {field('invoice_street', t('field.street'), { className: 'sm:col-span-4' })}
              {field('invoice_zip', t('field.zip'))}
              {field('invoice_city', t('field.city'), { className: 'sm:col-span-2' })}
              {field('invoice_country', t('field.country'), { fallback: default_country })}
            </div>
          </fieldset>
        ) : (
          <Button
            className="sm:col-span-2 sm:justify-self-start"
            onClick={() => setShowInvoiceAddress(true)}
          >
            <Plus className="size-4" />
            {t('field.invoiceAddress')}
          </Button>
        )}
      </section>

      <section className="mt-6">
        <h2 className="text-base">{t('customers.emails')}</h2>
        <ul className="mt-2 flex flex-col gap-2">
          {record.emails.map((email) => (
            <li
              key={email.id}
              className="flex items-center justify-between gap-3 rounded-card border border-line bg-surface px-3 py-2"
            >
              <span className="min-w-0 truncate">
                {email.email}
                <span className="ml-2 text-xs text-ink-faint">
                  {t(`emailType.${email.email_type}`)}
                </span>
              </span>
              <Button
                size="icon"
                variant="ghost"
                aria-label={t('action.delete')}
                onClick={() =>
                  deleteEmail.mutate(
                    { id: email.id },
                    {
                      onSuccess: () =>
                        client.invalidateQueries({
                          queryKey: getGetCustomerQueryKey(customerId),
                        }),
                    },
                  )
                }
              >
                <Trash2 className="size-4 text-danger" />
              </Button>
            </li>
          ))}
        </ul>

        <form
          // The server is authoritative for validation and its message is what the vet
          // reads; the browser's own bubble would block the submit and speak its language.
          noValidate
          className="mt-3 flex flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (!newEmail.trim()) return
            addEmail.mutate(
              { id: customerId, data: { email: newEmail, email_type: newEmailType } },
              {
                onSuccess: (updated) => {
                  setNewEmail('')
                  store(updated)
                },
              },
            )
          }}
        >
          <TextField
            label={t('field.email')}
            type="email"
            inputMode="email"
            value={newEmail}
            wrapperClassName="min-w-48 flex-1"
            error={addEmail.isError ? t('value.invalidEmail') : undefined}
            onChange={(event) => setNewEmail(event.target.value)}
          />
          <SelectField
            label={t('field.emailType')}
            value={newEmailType}
            wrapperClassName="w-36"
            onChange={(event) => setNewEmailType(event.target.value as EmailType)}
          >
            {EMAIL_TYPES.map((type) => (
              <option key={type} value={type}>
                {t(`emailType.${type}`)}
              </option>
            ))}
          </SelectField>
          <Button type="submit" variant="primary" disabled={addEmail.isPending}>
            {t('customers.addEmail')}
          </Button>
        </form>
      </section>

      <section className="mt-6">
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-base">{t('customers.patients')}</h2>
          <Button
            variant="accent"
            size="small"
            disabled={record.draft}
            onClick={() =>
              createPatient.mutate(
                { data: { customer_id: customerId } },
                {
                  onSuccess: async (patient) => {
                    await client.invalidateQueries({ queryKey: getListPatientsQueryKey() })
                    await navigate({
                      to: '/patients/$id',
                      params: { id: String(patient.id) },
                    })
                  },
                },
              )
            }
          >
            <Plus className="size-4" />
            {t('patients.new')}
          </Button>
        </div>
        <ul className="mt-2 flex flex-col gap-2">
          {(patients.data ?? []).map((patient) => (
            <li key={patient.id}>
              <button
                type="button"
                onClick={() =>
                  void navigate({ to: '/patients/$id', params: { id: String(patient.id) } })
                }
                className="w-full rounded-card border border-line bg-surface px-3 py-2 text-left hover:bg-cream-soft"
              >
                <span className="font-medium text-ink">{patient.name ?? t('patients.new')}</span>
                <span className="ml-2 text-sm text-ink-soft">{patient.species ?? ''}</span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    </div>
  )
}
