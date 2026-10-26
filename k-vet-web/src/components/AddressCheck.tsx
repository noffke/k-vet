import { useQuery } from '@tanstack/react-query'
import { CircleCheck, TriangleAlert } from 'lucide-react'
import { Trans, useTranslation } from 'react-i18next'
import { checkAddress } from '@/api/generated/endpoints'
import type { AddressCheckPart, AddressSuggestion } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { useOperatorConfig } from '@/lib/config'

interface AddressCheckProps {
  street: string | null | undefined
  zip: string | null | undefined
  city: string | null | undefined
  country: string
  /**
   * The vet changed this address on this visit to the page and has moved on from it. Opening a
   * customer costs no request; only a freshly typed address is worth a look.
   */
  active: boolean
  onApply: (suggestion: AddressSuggestion) => void
}

/**
 * Whether Google knows the address just typed — a hint, never a block: new buildings and farms
 * exist that Google does not. Strict all the same: an address Google writes differently is
 * offered as Google has it, with what differs in bold, and a part Google could not confirm is
 * named.
 */
export function AddressCheck({ street, zip, city, country, active, onApply }: AddressCheckProps) {
  const { t } = useTranslation()
  const { maps_enabled: mapsEnabled } = useOperatorConfig()
  const body = {
    street: street?.trim() ?? '',
    zip: zip?.trim() ?? '',
    city: city?.trim() ?? '',
    country,
  }
  const complete = body.street !== '' && body.zip !== '' && body.city !== ''
  const enabled = mapsEnabled && active && complete
  const check = useQuery({
    queryKey: ['address-check', body.street, body.zip, body.city, body.country],
    queryFn: () => checkAddress(body),
    enabled,
    // An address is asked about once a session; Google's answer does not change by the minute.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  })

  if (!enabled) return null
  if (check.isFetching) {
    return <p className="text-xs text-ink-faint">{t('addressCheck.checking')}</p>
  }
  switch (check.data?.verdict) {
    case 'found':
      return (
        <p className="flex items-center gap-1.5 text-xs text-ink-soft">
          <CircleCheck className="size-3.5 shrink-0 text-sage-deep" aria-hidden />
          {t('addressCheck.found')}
        </p>
      )
    case 'corrected': {
      const suggestion = check.data.suggestion
      if (!suggestion) return null
      return (
        <div
          className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink"
          aria-live="polite"
        >
          <TriangleAlert className="size-3.5 shrink-0 text-rust" aria-hidden />
          <span>
            <Trans
              i18nKey="addressCheck.corrected"
              components={{
                suggestion: <Suggestion suggestion={suggestion} changed={check.data.changed} />,
              }}
            />
          </span>
          <Button onClick={() => onApply(suggestion)}>{t('addressCheck.apply')}</Button>
        </div>
      )
    }
    case 'uncertain': {
      const parts = check.data.doubtful.map((part) => t(`addressCheck.part.${part}`))
      return (
        <p className="flex items-center gap-1.5 text-xs text-ink" aria-live="polite">
          <TriangleAlert className="size-3.5 shrink-0 text-rust" aria-hidden />
          {parts.length > 0
            ? t('addressCheck.uncertain', { parts: parts.join(', ') })
            : t('addressCheck.uncertainWhole')}
        </p>
      )
    }
    case 'not_found':
      return (
        <p className="flex items-center gap-1.5 text-xs text-rust" aria-live="polite">
          <TriangleAlert className="size-3.5 shrink-0" aria-hidden />
          {t('addressCheck.notFound')}
        </p>
      )
    default:
      // Unavailable or failed: nothing worth saying while she types.
      return null
  }
}

/** Google's version of the address, "Musterweg 5, 12345 Musterstadt", what differs in bold. */
function Suggestion({
  suggestion,
  changed,
}: {
  suggestion: AddressSuggestion
  changed: AddressCheckPart[]
}) {
  const part = (name: AddressCheckPart, text: string) =>
    changed.includes(name) ? <strong className="font-semibold">{text}</strong> : text
  return (
    <>
      {part('street', suggestion.street)}, {part('zip', suggestion.zip)}{' '}
      {part('city', suggestion.city)}
    </>
  )
}
