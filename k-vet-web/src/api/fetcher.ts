/**
 * Fetch mutator used by every generated endpoint.
 *
 * Adds session cookies, turns RFC 7807 problem documents into typed errors and keeps the
 * wire format locale-independent (formatting happens in `lib/format.ts`, never here).
 *
 * It is also where an expired session is noticed. Sessions end on inactivity, so the first
 * sign is a 401 on whatever the vet clicked next; without this the page simply renders
 * nothing and looks broken. Every protected endpoint funnels through here, so one check
 * catches all of them.
 */

export interface FieldError {
  field: string
  message: string
}

/** An error response from the API, carrying the problem document's details. */
export class ApiError extends Error {
  readonly status: number
  readonly fieldErrors: FieldError[]

  constructor(status: number, detail: string, fieldErrors: FieldError[]) {
    super(detail)
    this.name = 'ApiError'
    this.status = status
    this.fieldErrors = fieldErrors
  }

  /** Message for a single field, if the server reported one. */
  errorFor(field: string): string | undefined {
    return this.fieldErrors.find((error) => error.field === field)?.message
  }
}

interface ProblemDetails {
  detail?: string
  title?: string
  errors?: FieldError[]
}

/**
 * Everything under `/api/auth/` answers 401 for its own reasons — a wrong password, a
 * logout without a session — and none of them mean "your session just ended".
 */
const isAuthEndpoint = (url: string): boolean =>
  // Only the path decides; the origin is there so a relative URL parses at all.
  new URL(url, window.location.origin).pathname.startsWith('/api/auth/')

type UnauthorizedHandler = () => void

let onUnauthorized: UnauthorizedHandler | undefined

/**
 * Registered once by the composition root, which is the only place that knows the router
 * and the query cache. Left unset the mutator behaves exactly as it did before.
 */
export function setUnauthorizedHandler(handler: UnauthorizedHandler): void {
  onUnauthorized = handler
}

const isJson = (response: Response): boolean =>
  (response.headers.get('content-type') ?? '').includes('json')

export const apiFetch = async <T>(url: string, options: RequestInit = {}): Promise<T> => {
  const response = await fetch(url, {
    ...options,
    credentials: 'same-origin',
    headers: {
      Accept: 'application/json',
      ...(options.body !== undefined && !(options.body instanceof FormData)
        ? { 'Content-Type': 'application/json' }
        : {}),
      ...options.headers,
    },
  })

  if (!response.ok) {
    // The handler decides what happens next; the error is still thrown so the caller's own
    // error state stays truthful and nothing renders half-loaded data.
    if (response.status === 401 && !isAuthEndpoint(url)) onUnauthorized?.()

    let detail = `${response.status} ${response.statusText}`
    let fieldErrors: FieldError[] = []
    if (isJson(response)) {
      const problem = (await response.json().catch(() => ({}))) as ProblemDetails
      detail = problem.detail ?? problem.title ?? detail
      fieldErrors = problem.errors ?? []
    }
    throw new ApiError(response.status, detail, fieldErrors)
  }

  if (response.status === 204) return undefined as T
  if (!isJson(response)) return (await response.blob()) as T
  return (await response.json()) as T
}
