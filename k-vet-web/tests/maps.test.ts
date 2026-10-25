import { describe, expect, it } from 'vitest'
import { addressLine, directionsUrl } from '@/lib/maps'

const home = { street: 'Musterweg 5', zip: '12345', city: 'Musterstadt', country: 'DE' }

describe('addressLine', () => {
  it('reads street, postcode and town as one line', () => {
    expect(addressLine(home)).toBe('Musterweg 5, 12345 Musterstadt')
    expect(addressLine({ ...home, zip: null })).toBe('Musterweg 5, Musterstadt')
  })

  it('is no address without street or town', () => {
    expect(addressLine({ ...home, street: '  ' })).toBeNull()
    expect(addressLine({ ...home, city: null })).toBeNull()
  })
})

describe('directionsUrl', () => {
  it('plans the drive there, with the country by name', () => {
    expect(directionsUrl(home, 'de-DE')).toBe(
      'https://www.google.com/maps/dir/?api=1&destination=Musterweg%205%2C%2012345%20Musterstadt%2C%20Deutschland&travelmode=driving',
    )
    expect(directionsUrl(home, 'en-US')).toContain('Musterstadt%2C%20Germany')
  })

  it('encodes what an address may hold', () => {
    const url = directionsUrl(
      { street: 'Große Straße 3a/b', zip: '80331', city: 'München', country: 'DE' },
      'de-DE',
    )
    expect(url).toContain(`destination=${encodeURIComponent('Große Straße 3a/b, 80331 München')}`)
    expect(url && new URL(url).searchParams.get('destination')).toBe(
      'Große Straße 3a/b, 80331 München, Deutschland',
    )
  })

  it('sends a country it cannot name as given, and links nothing without an address', () => {
    expect(directionsUrl({ ...home, country: null }, 'de-DE')).toContain('Musterstadt&travelmode')
    expect(directionsUrl({ ...home, country: '??' }, 'de-DE')).toContain('Musterstadt%2C%20%3F%3F')
    expect(directionsUrl({ street: '', city: 'Musterstadt' }, 'de-DE')).toBeNull()
  })
})
