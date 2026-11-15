import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { getListAppointmentsQueryKey, useCreateAppointment } from '@/api/generated/endpoints'

/**
 * "Neuer Termin": creates the appointment at once and opens it — there is no create form to
 * abandon. From the appointment list it is empty; from a customer's page it is theirs (their only
 * animal on it), and from an animal it is for that animal alone. The detail page offers today's
 * date and waits for a time, which is what makes it complete.
 */
export function useNewAppointment() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const create = useCreateAppointment({
    mutation: {
      onSuccess: async (appointment) => {
        await client.invalidateQueries({ queryKey: getListAppointmentsQueryKey() })
        await navigate({ to: '/appointments/$id', params: { id: String(appointment.id) } })
      },
    },
  })
  return {
    open: (owner: { customerId?: number; patientId?: number } = {}) =>
      create.mutate({
        data: {
          starts_at: null,
          note: null,
          customer_id: owner.customerId ?? null,
          patient_id: owner.patientId ?? null,
        },
      }),
    pending: create.isPending,
  }
}
