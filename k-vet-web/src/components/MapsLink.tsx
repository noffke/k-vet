import { MapPin } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocaleFormat } from '@/lib/locale'
import { addressLine, directionsUrl, type MapsAddress } from '@/lib/maps'
import { cn } from '@/lib/utils'

/**
 * Opens Google Maps with the drive to `address` — the Maps app on the phone during a house
 * call. Renders nothing until the address has a street and a town.
 */
export function MapsLink({
  address,
  children,
  className,
}: {
  address: MapsAddress
  /** What the link says; "In Google Maps öffnen" when not given. */
  children?: ReactNode
  className?: string
}) {
  const { t } = useTranslation()
  const { locale } = useLocaleFormat()
  const href = directionsUrl(address, locale)
  if (!href) return null
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      title={t('maps.routeTo', { address: addressLine(address) })}
      className={cn(
        'inline-flex min-h-11 items-center gap-1.5 text-sm text-rust hover:underline sm:min-h-0',
        className,
      )}
    >
      <MapPin className="size-4 shrink-0" aria-hidden />
      {children ?? t('maps.open')}
    </a>
  )
}
