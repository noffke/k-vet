import type { APIRequestContext } from '@playwright/test'
import { E2E_PASSWORD, E2E_USERNAME } from './credentials'

/**
 * API-driven seeding (T023): each spec asks for the master data it needs instead of
 * driving another story's UI, so the story specs stay independently runnable.
 */

/** Logs the request context in; every later call reuses the session cookie. */
export async function login(request: APIRequestContext): Promise<void> {
  const response = await request.post('/api/auth/login', {
    data: { username: E2E_USERNAME, password: E2E_PASSWORD },
  })
  if (!response.ok()) throw new Error(`login failed: ${response.status()}`)
}

async function post<T>(request: APIRequestContext, url: string, data?: unknown): Promise<T> {
  const response = await request.post(url, data === undefined ? {} : { data })
  if (!response.ok()) {
    throw new Error(`POST ${url} failed: ${response.status()} ${await response.text()}`)
  }
  return (await response.json()) as T
}

async function patch<T>(request: APIRequestContext, url: string, data: unknown): Promise<T> {
  const response = await request.patch(url, { data })
  if (!response.ok()) {
    throw new Error(`PATCH ${url} failed: ${response.status()} ${await response.text()}`)
  }
  return (await response.json()) as T
}

export interface SeededCustomer {
  customerId: number
  lastName: string
}

/** A complete customer with one email address. */
export async function seedCustomer(request: APIRequestContext): Promise<SeededCustomer> {
  await login(request)
  const customer = await post<{ id: number }>(request, '/api/customers')
  // Unique per run: the specs share one database.
  const lastName = `Mustermann-${Date.now().toString().slice(-6)}`
  await patch(request, `/api/customers/${customer.id}`, {
    salutation: 'frau',
    first_name: 'Erika',
    last_name: lastName,
    home_street: 'Musterweg 5',
    home_zip: '12345',
    home_city: 'Musterstadt',
    phone: '030 12345678',
  })
  await post(request, `/api/customers/${customer.id}/emails`, {
    email: 'erika@example.com',
    email_type: 'private',
  })
  return { customerId: customer.id, lastName }
}

/** A complete patient belonging to `customerId`. */
export async function seedPatient(
  request: APIRequestContext,
  customerId: number,
  name = 'Bello',
): Promise<number> {
  const patient = await post<{ id: number }>(request, '/api/patients', {
    customer_id: customerId,
  })
  await patch(request, `/api/patients/${patient.id}`, {
    name,
    sex: 'male',
    species: 'Hund',
  })
  return patient.id
}

/** An appointment with a treatment, ready for billing lines. */
export async function seedTreatment(
  request: APIRequestContext,
  patientId: number,
): Promise<{ appointmentId: number; treatmentId: number }> {
  const appointment = await post<{ id: number }>(request, '/api/appointments', {
    starts_at: new Date().toISOString(),
  })
  const treatment = await post<{ id: number }>(
    request,
    `/api/appointments/${appointment.id}/treatments`,
    { patient_ids: [patientId] },
  )
  return { appointmentId: appointment.id, treatmentId: treatment.id }
}

export interface SeededDrug {
  drugId: number
  /** Unique per run, so a picker search cannot hit another spec's drug. */
  drugName: string
  /** Original packaging: a 100 ml bottle bought for 10.00 EUR net. */
  packagingId: number
  /** Subset packaging: 10 ml dispensed from the bottle. */
  subsetPackagingId: number
}

/** A complete drug with an original and a subset packaging, priced per AMPreisV. */
export async function seedDrug(request: APIRequestContext): Promise<SeededDrug> {
  await login(request)

  const supplier = await post<{ id: number }>(request, '/api/suppliers')
  await patch(request, `/api/suppliers/${supplier.id}`, { name: 'Großhandel GmbH' })
  const manufacturer = await post<{ id: number }>(request, '/api/manufacturers')
  await patch(request, `/api/manufacturers/${manufacturer.id}`, { name: 'Pharma AG' })

  const drug = await post<{ id: number }>(request, '/api/drugs')
  const drugName = `Amoxicillin-${Date.now().toString().slice(-6)}`
  await patch(request, `/api/drugs/${drug.id}`, {
    name: drugName,
    manufacturer_id: manufacturer.id,
    vat_percent: '19.000',
  })

  const original = await post<{ id: number }>(request, `/api/drugs/${drug.id}/packagings`, {
    kind: 'original',
  })
  await patch(request, `/api/packagings/${original.id}`, {
    unit: 'ml',
    quantity: '100',
    list_price_net: '10.00',
    supplier_id: supplier.id,
  })

  const subset = await post<{ id: number }>(request, `/api/drugs/${drug.id}/packagings`, {
    kind: 'subset',
  })
  await patch(request, `/api/packagings/${subset.id}`, { unit: 'ml', quantity: '10' })

  return { drugId: drug.id, drugName, packagingId: original.id, subsetPackagingId: subset.id }
}

export interface SeededService {
  serviceId: number
  /** Unique per run, so a picker search cannot hit another spec's service. */
  serviceName: string
}

/** A complete self-defined service, priced gross. */
export async function seedService(
  request: APIRequestContext,
  options: { grossPrice?: string; travelExpenses?: boolean } = {},
): Promise<SeededService> {
  await login(request)
  const service = await post<{ id: number }>(request, '/api/services', { type: 'self_defined' })
  const serviceName = `Untersuchung-${Date.now().toString().slice(-6)}`
  await patch(request, `/api/services/${service.id}`, {
    name: serviceName,
    vat_percent: '19.000',
    gross_price: options.grossPrice ?? '23.62',
    travel_expenses: options.travelExpenses ?? false,
  })
  return { serviceId: service.id, serviceName }
}

export interface SeededInvoice {
  invoiceId: number
  invoiceNumber: string
  treatmentId: number
}

/**
 * A treatment with one billed service line whose invoice is accepted — the state the
 * bookkeeping hand-off starts from. No recipients, so nothing is emailed.
 */
export async function seedAcceptedInvoice(request: APIRequestContext): Promise<SeededInvoice> {
  const { customerId } = await seedCustomer(request)
  const patientId = await seedPatient(request, customerId)
  const { treatmentId } = await seedTreatment(request, patientId)
  const { serviceId } = await seedService(request)

  await post(request, `/api/treatments/${treatmentId}/items`, {
    kind: 'service',
    service_id: serviceId,
    quantity: '1',
  })
  const invoice = await post<{ id: number; invoice_number: string }>(
    request,
    `/api/treatments/${treatmentId}/invoice`,
    {},
  )
  await post(request, `/api/invoices/${invoice.id}/accept`, { recipient_emails: [] })

  return { invoiceId: invoice.id, invoiceNumber: invoice.invoice_number, treatmentId }
}

/** Hands every pending invoice over, so a spec can count from a known slate. */
export async function clearPendingInvoices(request: APIRequestContext): Promise<void> {
  await login(request)
  await post(request, '/api/invoices/bulk-submit')
}
