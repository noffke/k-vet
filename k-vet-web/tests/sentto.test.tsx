import { render, screen } from '@testing-library/react'
import { beforeAll, describe, expect, it } from 'vitest'
import { SentTo } from '@/features/invoices/SentTo'
import i18n from '@/lib/i18n'

describe('SentTo', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('de-DE')
  })

  it('names every address the mail server took, CC and BCC included', () => {
    render(
      <SentTo
        to={['kundin@example.com', 'partner@example.com']}
        cc={['buchhaltung@example.com']}
        bcc={['archiv@example.com']}
      />,
    )

    expect(
      screen.getByText(
        'An kundin@example.com, partner@example.com · CC buchhaltung@example.com · BCC archiv@example.com',
      ),
    ).toBeInTheDocument()
  })

  it('leaves out CC and BCC that were not used', () => {
    render(<SentTo to={['kundin@example.com']} />)

    expect(screen.getByText('An kundin@example.com')).toBeInTheDocument()
  })

  it('draws nothing for an invoice no mail has left with', () => {
    const { container } = render(<SentTo to={[]} cc={['buchhaltung@example.com']} />)

    expect(container).toBeEmptyDOMElement()
  })
})
