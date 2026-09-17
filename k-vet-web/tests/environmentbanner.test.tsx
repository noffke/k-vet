import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { OperatorConfig } from '@/api/generated/model'
import { EnvironmentBanner } from '@/components/EnvironmentBanner'

const config = vi.hoisted(() => ({ current: {} as OperatorConfig }))

vi.mock('@/lib/config', () => ({
  useOperatorConfig: () => config.current,
}))

const base: OperatorConfig = {
  currency: 'EUR',
  vat_rates: ['19.000', '7.000'],
  default_country: 'DE',
  version: '1.2.3',
  environment_label: null,
}

describe('EnvironmentBanner', () => {
  beforeEach(() => {
    config.current = { ...base }
  })

  it('draws nothing on the practice’s own instance', () => {
    render(<EnvironmentBanner />)

    // Production must look exactly as it did; a banner there would be noise the vet learns to
    // ignore, which is how it stops working on the instance that needs it.
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('shows the operator’s own words, not a translated label', () => {
    config.current = {
      ...base,
      environment_label: 'TESTSYSTEM — keine echten Rechnungen',
    }

    render(<EnvironmentBanner />)

    expect(screen.getByRole('status')).toHaveTextContent('TESTSYSTEM — keine echten Rechnungen')
  })

  it('treats a blank label as no label', () => {
    // The backend already filters whitespace, but the banner is the last thing between a
    // blank string and an empty red stripe across production.
    config.current = { ...base, environment_label: '   ' }

    render(<EnvironmentBanner />)

    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })
})
