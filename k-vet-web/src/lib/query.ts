import { QueryClient } from '@tanstack/react-query'
import { ApiError } from '@/api/fetcher'

/**
 * One query client for the whole app — TanStack Query is the only server-state store
 * (research: state management). Filters and the current record live in the URL instead.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A single user on a local network: refetching on focus adds noise, not freshness.
      refetchOnWindowFocus: false,
      staleTime: 30_000,
      retry: (failureCount, error) => {
        // Never retry a rejected write or an expired session.
        if (error instanceof ApiError && error.status < 500) return false
        return failureCount < 2
      },
    },
    mutations: { retry: false },
  },
})

/**
 * Marks every cached query under a path stale — `/api/treatments/` for all treatments, whose
 * keys carry the id inside the path, so a key prefix cannot reach them.
 */
export function invalidatePath(client: QueryClient, path: string) {
  return client.invalidateQueries({
    predicate: (query) => {
      const [key] = query.queryKey
      return typeof key === 'string' && key.startsWith(path)
    },
  })
}
