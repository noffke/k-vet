import { keepPreviousData, type QueryKey, useInfiniteQuery } from '@tanstack/react-query'

/** How many rows a list asks for at a time. */
export const PAGE_SIZE = 50

/** Where the next page starts — or nothing, once a page came back short: there is no more. */
export function nextOffset(
  lastPage: readonly unknown[],
  pages: readonly (readonly unknown[])[],
): number | undefined {
  return lastPage.length < PAGE_SIZE ? undefined : pages.length * PAGE_SIZE
}

/**
 * The pages as one list, each record once. A record created or deleted while the vet pages
 * shifts every later offset, so a page can repeat the last row of the one before.
 */
export function mergePages<T extends { id: number }>(pages: readonly (readonly T[])[]): T[] {
  const seen = new Set<number>()
  const rows: T[] = []
  for (const page of pages) {
    for (const row of page) {
      if (!seen.has(row.id)) {
        seen.add(row.id)
        rows.push(row)
      }
    }
  }
  return rows
}

/** What a paged list hands its page: the rows so far, and the way to the next ones. */
export interface PagedList<T> {
  rows: T[]
  hasMore: boolean
  loadMore: () => void
  loadingMore: boolean
  isPending: boolean
  isError: boolean
}

/**
 * A list read page by page through a generated fetcher (`listAppointments`, …) — the server
 * stops at its default limit, and a list that silently ended there looked complete.
 *
 * The key is the generated list key plus `'paged'`, so the invalidations that already name the
 * list's key (`getListAppointmentsQueryKey()`) reach it as a prefix and refetch every page
 * loaded so far.
 */
export function usePagedList<
  T extends { id: number },
  P extends { limit?: number; offset?: number },
>(
  queryKey: QueryKey,
  fetchPage: (params: P, options?: RequestInit) => Promise<T[]>,
  params: Omit<P, 'limit' | 'offset'>,
  options: { enabled?: boolean } = {},
): PagedList<T> {
  const query = useInfiniteQuery({
    queryKey: [...queryKey, 'paged'],
    queryFn: ({ pageParam, signal }) =>
      fetchPage({ ...params, limit: PAGE_SIZE, offset: pageParam } as P, { signal }),
    initialPageParam: 0,
    getNextPageParam: nextOffset,
    // A new search keeps the old rows on screen until its first page is in.
    placeholderData: keepPreviousData,
    enabled: options.enabled ?? true,
  })
  return {
    rows: mergePages(query.data?.pages ?? []),
    hasMore: query.hasNextPage,
    loadMore: () => void query.fetchNextPage(),
    loadingMore: query.isFetchingNextPage,
    isPending: query.isPending,
    isError: query.isError,
  }
}
