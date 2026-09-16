import { Command } from 'cmdk'
import { UserRound, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useListCustomers } from '@/api/generated/endpoints'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

const SEARCH_DEBOUNCE_MS = 200

interface CustomerPickerProps {
  /** The customer currently chosen, if any. */
  customerId: number | null
  /** Shown instead of the search box once one is chosen. */
  customerName: string | null
  onPick: (customerId: number) => void
  onClear: () => void
  /** Once a treatment exists the answer is settled — changing it would move its animals. */
  locked?: boolean | undefined
}

/**
 * Whose visit an appointment is.
 *
 * It has to be answered before animals can be attached, because it is what limits them to one
 * customer (FR-027, issues.md 7) — an appointment with no customer used to offer every animal
 * in the practice until the first was picked.
 */
export function CustomerPicker({
  customerId,
  customerName,
  onPick,
  onClear,
  locked,
}: CustomerPickerProps) {
  const { t } = useTranslation()
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(term), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [term])

  const customers = useListCustomers(
    { q: debounced || undefined },
    { query: { enabled: customerId === null } },
  )
  const options = (customers.data ?? []).filter((customer) => !customer.draft)

  if (customerId !== null) {
    return (
      <div className="flex items-center gap-2">
        <span className="flex items-center gap-1.5 rounded-full border border-line bg-surface py-1 pr-1 pl-3">
          <UserRound className="size-3.5 text-rust" aria-hidden />
          <span className="text-sm text-ink">{customerName ?? '—'}</span>
          {locked ? null : (
            // It takes the customer off the record; it does not delete the customer. Calling
            // it "Löschen" told a screen reader otherwise, and put a second button of that
            // name beside the one that really does delete.
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('action.remove')}
              className="size-7 sm:size-7"
              onClick={onClear}
            >
              <X className="size-3.5" />
            </Button>
          )}
        </span>
      </div>
    )
  }

  return (
    <Command shouldFilter={false} className="rounded-card border border-line-strong bg-surface">
      <Command.Input
        value={term}
        onValueChange={setTerm}
        placeholder={t('appointments.pickCustomer')}
        className={cn(
          'w-full rounded-t-card border-b border-line bg-transparent px-3 py-2.5 text-ink',
          'placeholder:text-ink-faint focus:outline-none',
        )}
      />
      <Command.List className="max-h-48 overflow-y-auto p-1">
        <Command.Empty className="px-2 py-3 text-sm text-ink-faint">
          {t('list.empty')}
        </Command.Empty>
        {options.map((customer) => (
          <Command.Item
            key={customer.id}
            value={String(customer.id)}
            onSelect={() => {
              setTerm('')
              onPick(customer.id)
            }}
            className="flex cursor-pointer items-center justify-between gap-2 rounded-control px-2 py-2 text-sm text-ink data-[selected=true]:bg-cream-soft"
          >
            <span>{[customer.first_name, customer.last_name].filter(Boolean).join(' ')}</span>
            <span className="text-xs text-ink-faint">{customer.home_city ?? ''}</span>
          </Command.Item>
        ))}
      </Command.List>
    </Command>
  )
}
