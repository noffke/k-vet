import { queryOptions, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { getSessionInfoQueryKey, login, logout, sessionInfo } from '@/api/generated/endpoints'
import type { LoginRequest } from '@/api/generated/model'

/** The session the whole app hangs off; the router guard awaits it before rendering. */
export function sessionQueryOptions() {
  return queryOptions({
    queryKey: getSessionInfoQueryKey(),
    queryFn: ({ signal }) => sessionInfo({ signal }),
    staleTime: 60_000,
  })
}

export function useLogin() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (credentials: LoginRequest) => login(credentials),
    onSuccess: (session) => {
      client.setQueryData(getSessionInfoQueryKey(), session)
    },
  })
}

export function useLogout() {
  const client = useQueryClient()
  const navigate = useNavigate()
  return useMutation({
    mutationFn: () => logout(),
    onSuccess: async () => {
      client.clear()
      await navigate({ to: '/login' })
    },
  })
}
