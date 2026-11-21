import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { type APIRequestContext, expect, test } from '../fixtures/test'
import {
  doc,
  seedAcceptedInvoice,
  seedCustomer,
  seedDrug,
  seedFailedMail,
  seedPatient,
  seedQuote,
  seedService,
  seedTreatment,
} from '../fixtures/seed'
import { editValue, expectNoSidewaysScroll, openPosition, signIn } from './helpers'

/**
 * No screen scrolls sideways, on the laptop or the phone (the developer, 2026-11-21) — which on a
 * phone would also shrink Chrome's page scale and move the fixed tab bar. Every route the router
 * knows is opened with the long data real practices have, once its data is in, and measured.
 * `navigate()` checks the same on every navigation, but only with the short names other specs
 * seed; this is where a long name, a long e-mail address or a long unit is put to it.
 */
const here = dirname(fileURLToPath(import.meta.url))
const stamp = () => String(Date.now() % 1e6)

const LONG = {
  lastName: 'Müller-Lüdenscheidt-von Hohenzollern-Sigmaringen',
  street: 'Hauptstraße 123, Hinterhaus, Aufgang C, 3. Obergeschoss links',
  email: 'anna-katharina.mueller-luedenscheidt.von-hohenzollern@tierarztpraxis-mit-sehr-langem-namen.de',
  animal: 'Prinzessin Anastasia von der Blumenwiese am Waldesrand',
  drug: 'Amoxicillin/Clavulansäure 400/100 mg Filmtabletten für Hunde und Katzen',
  // Names no other spec searches for ("Beratung im einzelnen Fall", "Wegegeld"): the specs of
  // a worker share its database, and a picker search would find these first.
  service: 'Ganzkörperuntersuchung mit ausführlicher Anamnese, Befundbesprechung und Dokumentation',
  company: 'Großhandelsgesellschaft für Tierarzneimittel und Praxisbedarf mbH & Co. KG',
}

async function post<T>(request: APIRequestContext, url: string, data: unknown = {}): Promise<T> {
  const response = await request.post(url, { data })
  expect(response.ok(), `${url}: ${await response.text()}`).toBe(true)
  return (await response.json()) as T
}

async function patch(request: APIRequestContext, url: string, data: unknown): Promise<void> {
  const response = await request.patch(url, { data })
  expect(response.ok(), `${url}: ${await response.text()}`).toBe(true)
}

/** Every route of the router, by its path pattern. */
function routerPaths(): string[] {
  const source = readFileSync(resolve(here, '../../k-vet-web/src/router.tsx'), 'utf8')
  return [...source.matchAll(/path: '([^']+)'/g)].map((match) => match[1] as string)
}

test('no screen scrolls sideways, with long names everywhere', async ({ page, request }) => {
  test.setTimeout(180_000)

  // ── The long data ─────────────────────────────────────────────────────────────────────────
  const { customerId } = await seedCustomer(request, { street: LONG.street })
  await patch(request, `/api/customers/${customerId}`, {
    first_name: 'Anna-Katharina',
    last_name: `${LONG.lastName}-${stamp()}`,
  })
  await post(request, `/api/customers/${customerId}/emails`, {
    email: LONG.email,
    email_type: 'work',
  })
  const patientId = await seedPatient(request, customerId, `${LONG.animal} ${stamp()}`)
  const { treatmentId, appointmentId } = await seedTreatment(request, patientId)

  const drug = await seedDrug(request, { unit: 'Tablette' })
  const drugName = `${LONG.drug} ${stamp()}`
  await patch(request, `/api/drugs/${drug.drugId}`, { name: drugName })
  const lot = await post<{ id: number }>(
    request,
    `/api/packagings/${drug.packagingId}/stock-intakes`,
    { packages_received: 2, batch_number: `CH-${stamp()}-LANGE-CHARGENBEZEICHNUNG-2026` },
  )
  const { serviceId, serviceName } = await seedService(request, { name: LONG.service })
  const travel = await seedService(request, {
    name: 'Fahrtkosten Langstrecke',
    netPrice: '13.00',
    travelExpenses: true,
  })
  for (const line of [
    { kind: 'drug_packaging', drug_packaging_id: drug.packagingId },
    { kind: 'service', service_id: serviceId, quantity: '1' },
    { kind: 'service', service_id: travel.serviceId, quantity: '1' },
  ]) {
    await post(request, `/api/treatments/${treatmentId}/items`, line)
  }
  await post(request, `/api/appointments/${appointmentId}/reminders`, {
    drug_packaging_id: drug.packagingId,
  })
  await patch(request, `/api/patient-treatments/${(await (await request.get(`/api/treatments/${treatmentId}`)).json()).patients[0].id}`, {
    treatment_reason: doc(`${LONG.service}. ${LONG.street}.`),
  })

  const manufacturer = await post<{ id: number }>(request, '/api/manufacturers')
  await patch(request, `/api/manufacturers/${manufacturer.id}`, { name: `${LONG.company} ${stamp()}` })
  const supplier = await post<{ id: number }>(request, '/api/suppliers')
  await patch(request, `/api/suppliers/${supplier.id}`, {
    name: `${LONG.company} ${stamp()}`,
    addr_street: LONG.street,
    addr_zip: '12345',
    addr_city: 'Garmisch-Partenkirchen',
  })
  const template = await post<{ id: number }>(request, '/api/treatment-templates')
  await patch(request, `/api/treatment-templates/${template.id}`, {
    name: `${LONG.service} ${stamp()}`,
  })
  await post(request, `/api/treatment-templates/${template.id}/items`, {
    kind: 'drug_packaging',
    drug_packaging_id: drug.packagingId,
  })
  const block = await post<{ id: number }>(request, '/api/text-blocks')
  await patch(request, `/api/text-blocks/${block.id}`, {
    name: `${LONG.service} ${stamp()}`,
    content: doc(`${LONG.service} ${LONG.email}`),
  })
  const { quoteId } = await seedQuote(request, customerId, `${LONG.service} ${stamp()}`, [
    { kind: 'service', id: serviceId, quantity: '1' },
    { kind: 'drug_packaging', id: drug.packagingId, quantity: '10' },
  ])
  // This visit's Wegegeld has no km yet, so its invoice page says so; a second visit of the
  // same customer has its invoice created — and another one is released, its mail refused.
  const second = await seedTreatment(request, patientId)
  await post(request, `/api/treatments/${second.treatmentId}/items`, {
    kind: 'drug_packaging',
    drug_packaging_id: drug.packagingId,
  })
  await post(request, `/api/treatments/${second.treatmentId}/invoice`)
  const released = await seedAcceptedInvoice(request)
  await seedFailedMail(released.invoiceId, LONG.email)

  // ── Every route ───────────────────────────────────────────────────────────────────────────
  const routes: Record<string, string> = {
    '/login': '/login',
    '/': '/',
    '/invoices': '/invoices',
    '/settings': '/settings',
    '/appointments': '/appointments',
    '/packing-list': '/packing-list',
    '/appointments/$id': `/appointments/${appointmentId}`,
    '/customers': `/customers?q=${encodeURIComponent('Müller-Lüdenscheidt')}`,
    '/customers/$id': `/customers/${customerId}`,
    '/patients/$id': `/patients/${patientId}`,
    '/pharmacy': '/pharmacy',
    '/pharmacy/delivery': '/pharmacy/delivery',
    '/pharmacy/$id': `/pharmacy/${drug.drugId}`,
    '/pharmacy/$id/intake': `/pharmacy/${drug.drugId}/intake`,
    '/lots/$id': `/lots/${lot.id}`,
    '/lots/$id/correction': `/lots/${lot.id}/correction`,
    '/masterdata': '/masterdata',
    '/masterdata/manufacturers/$id': `/masterdata/manufacturers/${manufacturer.id}`,
    '/masterdata/suppliers/$id': `/masterdata/suppliers/${supplier.id}`,
    '/services': '/services',
    '/services/$id': `/services/${serviceId}`,
    '/quote': `/quote?id=${quoteId}`,
    '/templates': '/templates',
    '/templates/$id': `/templates/${template.id}`,
    '/text-blocks': '/text-blocks',
    '/text-blocks/$id': `/text-blocks/${block.id}`,
    '/treatments/$id': `/treatments/${treatmentId}`,
    '/treatments/$id/invoice': `/treatments/${treatmentId}/invoice`,
    '/treatments/$id/invoice/accept': `/treatments/${second.treatmentId}/invoice/accept`,
    '/treatments/$id/invoice/send': `/treatments/${released.treatmentId}/invoice/send`,
  }
  // A page the router gained without an entry here would go unmeasured.
  expect(Object.keys(routes).sort()).toEqual(routerPaths().sort())

  await signIn(page)
  for (const [route, url] of Object.entries(routes)) {
    await test.step(route, async () => {
      await page.goto(url)
      await page.waitForLoadState('networkidle')
      await expectNoSidewaysScroll(page)
    })
  }

  // The positions in every state: one opened, one value being typed.
  await page.goto(`/appointments/${appointmentId}`)
  await page.waitForLoadState('networkidle')
  await openPosition(page, drugName.slice(0, 40))
  await expectNoSidewaysScroll(page)
  await editValue(page, serviceName.slice(0, 40), 'Faktor')
  await expectNoSidewaysScroll(page)
  await editValue(page, travel.serviceName, 'Kilometer')
  await expectNoSidewaysScroll(page)
})
