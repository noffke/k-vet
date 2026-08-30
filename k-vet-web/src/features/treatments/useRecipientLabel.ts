import { useTranslation } from 'react-i18next'
import { useGetCustomer } from '@/api/generated/endpoints'

/**
 * Labels an invoice recipient with the type the customer has stored for it — "privat",
 * "geschäftlich" — because which of several addresses is which is the whole decision being made
 * on the accept and send screens.
 *
 * The type comes from the customer rather than the treatment: `Treatment.customer_emails` is a
 * list of plain strings on the wire, and widening that contract to answer a question only these
 * two screens ask would reach every other consumer of it.
 *
 * An address the invoice went to once but which has since been removed from the customer gets no
 * type and shows as itself, which is honest — the practice no longer has one for it.
 */
export function useRecipientLabel(customerId: number | null | undefined) {
  const { t } = useTranslation()
  const customer = useGetCustomer(customerId ?? 0, { query: { enabled: Boolean(customerId) } })

  return (email: string): string => {
    const type = customer.data?.emails.find((stored) => stored.email === email)?.email_type
    return type ? `${email} · ${t(`emailType.${type}`)}` : email
  }
}
