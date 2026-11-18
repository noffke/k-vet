import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type {
  Appointment,
  AtRisk,
  AtRiskEntry,
  Dashboard,
  QueuedEmail,
} from '@/api/generated/model'
import { DashboardPage } from '@/features/dashboard/DashboardPage'
import i18n from '@/lib/i18n'

const answer: { data: Dashboard | undefined; isPending: boolean } = {
  data: undefined,
  isPending: true,
}

const today: { data: Appointment[] } = { data: [] }

vi.mock('@/api/generated/endpoints', () => ({
  useGetDashboard: () => ({ ...answer, isError: false }),
  useListAppointments: () => ({ data: today.data, isSuccess: true }),
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

function released(customer: string, email_queue: QueuedEmail | null): AtRiskEntry {
  return {
    treatment_id: customer.length,
    invoice_number: `2026-${customer.length}`,
    date: '2026-10-20',
    customer_name: customer,
    patients: ['Bello'],
    total_gross: '28.11',
    email_queue,
  }
}

function mail(changes: Partial<QueuedEmail>): QueuedEmail {
  return {
    id: 1,
    invoice_id: 1,
    recipients: ['kundin@example.com'],
    queued_at: '2026-10-20T08:00:00Z',
    send_after: '2026-10-20T08:00:00Z',
    attempts: 0,
    refused: false,
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
              email_queue: null,
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

  // "freigegeben, aber nie beim Kunden angekommen" was untrue of a mail scheduled for later
  // (issues.md): each row says where its mail is instead, and a failed one says why.
  it('tells a scheduled, a failed and an unqueued mail apart, without the old explanation', () => {
    show(
      dashboard({
        unsent: {
          count: 3,
          entries: [
            released(
              'Gerda Gescheitert',
              mail({
                last_error: 'permanent error (550): no such user',
                refused: true,
                gave_up_at: '2026-10-20T08:05:00Z',
              }),
            ),
            released('Paula Plan', mail({ send_after: '2099-01-02T07:00:00Z' })),
            released('Otto Ohnemail', null),
          ],
        },
      }),
    )
    expect(
      screen.getByText('E-Mail-Versand abgebrochen: permanent error (550): no such user'),
    ).toBeInTheDocument()
    expect(screen.getByText(/^E-Mail geplant: 02\.01\.2099/)).toBeInTheDocument()
    expect(screen.getByText('Keine E-Mail geplant')).toBeInTheDocument()
    expect(screen.queryByText(/nie beim Kunden angekommen/)).not.toBeInTheDocument()
  })

  it('says once that nothing is waiting', () => {
    show(dashboard())
    expect(screen.getByText('Alles erledigt — nichts wartet.')).toBeInTheDocument()
    // Only the day's appointments, which are always there.
    expect(
      screen.queryAllByRole('heading', { level: 2 }).map((heading) => heading.textContent),
    ).toEqual(['Heute'])
  })

  it('shows neither sections nor "all done" while it loads', () => {
    show(undefined)
    expect(screen.queryByText('Alles erledigt — nichts wartet.')).not.toBeInTheDocument()
    expect(
      screen.queryAllByRole('heading', { level: 2 }).map((heading) => heading.textContent),
    ).toEqual(['Heute'])
  })

  // Today's appointments, each a link to it, with the chips Termine shows.
  it("lists today's appointments, or says there are none", () => {
    today.data = []
    show(dashboard())
    expect(screen.getByText('Heute keine Termine')).toBeInTheDocument()
    today.data = [
      {
        id: 3,
        starts_at: '2026-11-18T08:30:00Z',
        note: null,
        draft: false,
        missing_fields: [],
        customer_id: 1,
        customer_name: 'Erika Mustermann',
        address: { street: 'Musterweg 5', zip: '12345', city: 'Musterstadt', country: 'DE' },
        patient_names: ['Bello'],
        treatment_id: 3,
        unbilled: true,
        pdf_stale: false,
        reminder_count: 0,
        from_phone: false,
        calendar_hidden: false,
        created_at: '2026-11-18T07:00:00Z',
        updated_at: '2026-11-18T07:00:00Z',
      } as Appointment,
    ]
    show(dashboard())
    expect(screen.getByText('Erika Mustermann')).toBeInTheDocument()
    expect(screen.getByText('Musterweg 5, 12345 Musterstadt')).toBeInTheDocument()
    expect(screen.getByText('Nicht abgerechnet')).toBeInTheDocument()
    today.data = []
  })
})
