import { useTranslation } from 'react-i18next'
import { SelectField } from '@/components/ui/field'
import { useOperatorConfig } from '@/lib/config'
import { useLocaleFormat } from '@/lib/locale'

interface VatSelectProps {
  value: string | null | undefined
  onChange: (value: string | null) => void
  error?: string | undefined
}

/**
 * The VAT rate of a drug or a service, offered from the rates the operator configured — not
 * from a hardcoded 19/7, which would be wrong the day the practice bills in another country
 * or the legislator moves a rate.
 */
export function VatSelect({ value, onChange, error }: VatSelectProps) {
  const { t } = useTranslation()
  const { percent } = useLocaleFormat()
  const { vat_rates } = useOperatorConfig()

  return (
    <SelectField
      label={t('field.vat')}
      defaultValue={value ?? ''}
      error={error}
      onChange={(event) => onChange(event.target.value || null)}
    >
      <option value="">—</option>
      {vat_rates.map((rate) => (
        <option key={rate} value={rate}>
          {percent(rate)}
        </option>
      ))}
    </SelectField>
  )
}
