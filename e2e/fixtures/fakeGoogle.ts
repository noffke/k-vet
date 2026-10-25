import { createServer, type Server } from 'node:http'

/**
 * A stand-in for Google's Routes and Address Validation APIs: the binary is pointed at it with
 * `KVET_MAPS_ENDPOINT`, so the suite runs with a Maps key and no network.
 */

/**
 * The street the fake knows a route to — 11.902 km, the shorter of two routes, so 12 whole km.
 * Every other destination has none, so a spec that adds a Wegegeld line without thinking of
 * Google keeps an empty kilometre field, as without a key.
 */
export const ROUTED_STREET = 'Wegegeldweg'
export const ROUTED_KM = '12'

/** Misspelled like this, the address check offers the street spelled right. */
export const MISSPELLED_STREET = 'Musterwg'
/** A house number the address check cannot confirm. */
export const UNKNOWN_HOUSE = '99'

const routes = {
  routes: [
    { distanceMeters: 12437, routeLabels: ['DEFAULT_ROUTE'] },
    { distanceMeters: 11902, routeLabels: ['SHORTER_DISTANCE'] },
  ],
  geocodingResults: { destination: { type: ['street_address'] } },
}

function validation(street: string) {
  const unknown = street.endsWith(` ${UNKNOWN_HOUSE}`)
  const corrected = street.includes(MISSPELLED_STREET)
  const line = corrected ? street.replace(MISSPELLED_STREET, 'Musterweg') : street
  return {
    result: {
      verdict: { validationGranularity: unknown ? 'ROUTE' : 'PREMISE', addressComplete: true },
      address: {
        postalAddress: {
          regionCode: 'DE',
          postalCode: '12345',
          locality: 'Musterstadt',
          addressLines: [line],
        },
        addressComponents: [
          {
            componentType: 'route',
            confirmationLevel: unknown ? 'UNCONFIRMED_BUT_PLAUSIBLE' : 'CONFIRMED',
            spellCorrected: corrected,
          },
        ],
      },
    },
  }
}

export function startFakeGoogle(): Promise<{ server: Server; url: string }> {
  const server = createServer((request, response) => {
    let raw = ''
    request.on('data', (chunk: Buffer) => {
      raw += chunk.toString('utf8')
    })
    request.on('end', () => {
      let body: {
        destination?: { address?: string }
        address?: { addressLines?: string[] }
      } = {}
      try {
        body = JSON.parse(raw)
      } catch {
        // An unreadable request gets the same empty answer as an unknown one.
      }
      response.setHeader('content-type', 'application/json')
      if (request.url?.endsWith(':computeRoutes')) {
        const destination = body.destination?.address ?? ''
        response.end(JSON.stringify(destination.includes(ROUTED_STREET) ? routes : {}))
        return
      }
      response.end(JSON.stringify(validation(body.address?.addressLines?.[0] ?? '')))
    })
  })
  return new Promise((done, fail) => {
    server.once('error', fail)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      if (address === null || typeof address === 'string') {
        fail(new Error('the fake Google has no port'))
        return
      }
      done({ server, url: `http://127.0.0.1:${address.port}` })
    })
  })
}
