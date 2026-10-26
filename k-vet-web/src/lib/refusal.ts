import { ApiError } from '@/api/fetcher'

/**
 * The i18n key for why the server refused a write: its first field message, else the generic
 * one. (The generated hooks type their error as void; what the fetcher throws is an ApiError.)
 */
export function refusalKey(error: unknown): string {
  return (error instanceof ApiError ? error.fieldErrors[0]?.message : undefined) ?? 'error.generic'
}
