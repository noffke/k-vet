import { useGetConfig } from '@/api/generated/endpoints'
import type { OperatorConfig } from '@/api/generated/model'

/**
 * What the operator decided in `config.toml`: the currency, the VAT rates the practice may
 * charge, and the country to assume. It cannot change while the app is open, so it is fetched
 * once and kept — and until it arrives the screens fall back to the German defaults the
 * backend itself uses, rather than rendering an empty select.
 */
const FALLBACK: OperatorConfig = {
  currency: 'EUR',
  vat_rates: ['19.000', '7.000'],
  default_country: 'DE',
  // Before the answer arrives we do not know which instance this is, and guessing would be the
  // wrong way round: an unlabelled fallback on staging is exactly the confusion the label
  // exists to prevent. The banner simply waits.
  version: '',
  environment_label: null,
}

export function useOperatorConfig(): OperatorConfig {
  const config = useGetConfig({ query: { staleTime: Number.POSITIVE_INFINITY } })
  return config.data ?? FALLBACK
}
