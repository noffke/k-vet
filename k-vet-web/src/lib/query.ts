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
