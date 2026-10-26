import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { AtRisk, Dashboard } from '@/api/generated/model'
import { DashboardPage } from '@/features/dashboard/DashboardPage'
import i18n from '@/lib/i18n'

const answer: { data: Dashboard | undefined; isPending: boolean } = {
  data: undefined,
  isPending: true,
}

vi.mock('@/api/generated/endpoints', () => ({
  useGetDashboard: () => ({ ...answer, isError: false }),
  // The operator's currency: absent, money is formatted with the German defaults.
  useGetConfig: () => ({ data: undefined }),
}))
// The header reads the router's history for its back link; the page under test is the grid.
vi.mock('@/components/PageHeader', () => ({
  PageHeader: ({ title }: { title: string }) => <h1>{title}</h1>,
}))
vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, className }: { children: ReactNode; className?: string }) => (
    <a href="/" className={className}>
      {children}
    </a>
  ),
}))

const none: AtRisk = { count: 0, entries: [] }

function dashboard(changes: Partial<Dashboard> = {}): Dashboard {
  return {
    pending_invoice_count: 0,
    unbilled: none,
    unreleased: none,
    unsent: none,
    stale_pdfs: none,
    expiring_lots: [],
    failed_mails: [],
    ...changes,
  }
}

function show(data: Dashboard | undefined) {
  answer.data = data
  answer.isPending = data === undefined
  return render(<DashboardPage />)
}

/**
 * The dashboard shows what has something in it (issues.md): a section with nothing to do is
 * left out rather than shown with a "0", and with nothing anywhere the page says so once.
 */
describe('DashboardPage', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('de-DE')
  })

  it('leaves out the sections with nothing in them', () => {
    show(
      dashboard({
        unsent: {
          count: 1,
          entries: [
            {
              treatment_id: 7,
              invoice_number: '2026-0007',
              date: '2026-10-20',
              customer_name: 'Erika Mustermann',
              patients: ['Bello'],
              total_gross: '28.11',
              mail_scheduled_for: null,
            },
          ],
        },
      }),
    )
    expect(screen.getByRole('heading', { name: /Nicht versendet/ })).toBeInTheDocument()
    expect(screen.getByText('Erika Mustermann')).toBeInTheDocument()
    for (const empty of [
      /Nicht abgerechnet/,
      /Nicht freigegeben/,
      /PDF veraltet/,
      /Fehlgeschlagene E-Mails/,
      /Chargen/,
    ]) {
      expect(screen.queryByRole('heading', { name: empty })).not.toBeInTheDocument()
    }
    expect(screen.queryByText(/Rechnungen für die Buchhaltung/)).not.toBeInTheDocument()
    expect(screen.queryByText('Alles erledigt — nichts wartet.')).not.toBeInTheDocument()
  })

  it('says once that nothing is waiting', () => {
    show(dashboard())
    expect(screen.getByText('Alles erledigt — nichts wartet.')).toBeInTheDocument()
    expect(screen.queryAllByRole('heading', { level: 2 })).toHaveLength(0)
  })

  it('shows neither sections nor "all done" while it loads', () => {
    show(undefined)
    expect(screen.queryByText('Alles erledigt — nichts wartet.')).not.toBeInTheDocument()
    expect(screen.queryAllByRole('heading', { level: 2 })).toHaveLength(0)
  })
})
