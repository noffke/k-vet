/**
 * Fetch mutator used by every generated endpoint.
 *
 * Adds session cookies, turns RFC 7807 problem documents into typed errors and keeps the
 * wire format locale-independent (formatting happens in `lib/format.ts`, never here).
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
