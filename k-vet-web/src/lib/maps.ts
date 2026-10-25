/**
 * Links to Google Maps. A link is not a request: it needs no key and sends nothing until the vet
 * taps it, so it is offered whether or not the server has a Maps key.
 */

/** An address as the customer record holds it; `country` is ISO 3166-1 alpha-2. */
export interface MapsAddress {
  street?: string | null | undefined
  zip?: string | null | undefined
  city?: string | null | undefined
  country?: string | null | undefined
}

/** "Musterweg 5, 12345 Musterstadt" — or null without street and town, which is no address. */
export function addressLine(address: MapsAddress): string | null {
  const street = address.street?.trim()
  const city = address.city?.trim()
  if (!street || !city) return null
  const place = [address.zip?.trim(), city].filter(Boolean).join(' ')
  return `${street}, ${place}`
}

/** The country's name in `locale`, which Google reads more surely than a two-letter code. */
function countryName(code: string, locale: string): string {
  try {
    return new Intl.DisplayNames([locale], { type: 'region' }).of(code.toUpperCase()) ?? code
  } catch {
    return code
  }
}

/**
 * Google Maps planning the drive to `address`: on the phone the Maps app with the route from
 * where the vet is, on the laptop the website. Null without street and town.
 */
export function directionsUrl(address: MapsAddress, locale: string): string | null {
  const line = addressLine(address)
  if (!line) return null
  const country = address.country?.trim()
  const destination = country ? `${line}, ${countryName(country, locale)}` : line
  return `https://www.google.com/maps/dir/?api=1&destination=${encodeURIComponent(destination)}&travelmode=driving`
}
