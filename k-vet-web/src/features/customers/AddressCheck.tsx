import { useQuery } from '@tanstack/react-query'
import { CircleCheck, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { checkAddress } from '@/api/generated/endpoints'
import type { AddressSuggestion } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { useOperatorConfig } from '@/lib/config'
import { addressLine } from '@/lib/maps'

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
 * exist that Google does not. A spelling Google writes differently can be taken over.
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
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink" aria-live="polite">
          <TriangleAlert className="size-3.5 shrink-0 text-rust" aria-hidden />
          <span>{t('addressCheck.corrected', { address: addressLine(suggestion) })}</span>
          <Button onClick={() => onApply(suggestion)}>{t('addressCheck.apply')}</Button>
        </div>
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
