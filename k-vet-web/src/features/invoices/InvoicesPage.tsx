import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Ban, Download, FileText, PackageCheck, Send } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getInvoicePdfUrl,
  getListInvoicesQueryKey,
  getPendingInvoicePdfsUrl,
  useBulkSubmitInvoices,
  useCancelInvoice,
  useListInvoices,
  useMarkInvoicePosted,
  useSubmitInvoice,
} from '@/api/generated/endpoints'
import type { Invoice } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DataList, type DataListColumn } from '@/components/DataList'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { CheckboxField } from '@/components/ui/field'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

/**
 * The invoice overview and the monthly bookkeeping hand-off (FR-034).
 *
 * Invoices waiting for the bookkeeper sit in their own block on top: that is the work the
 * vet opens this page for. Below it the full list, cancelled invoices only on request.
 */
export function InvoicesPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const { money, date } = useLocaleFormat()

  const [search, setSearch] = useState('')
  const [showCancelled, setShowCancelled] = useState(false)
  const [handOverOpen, setHandOverOpen] = useState(false)
  const [cancelling, setCancelling] = useState<Invoice | null>(null)

  const pending = useListInvoices({ pending: true })
  const invoices = useListInvoices({
    q: search || undefined,
    cancelled: showCancelled || undefined,
  })

  const refresh = () => client.invalidateQueries({ queryKey: getListInvoicesQueryKey() })
  const submit = useSubmitInvoice({ mutation: { onSuccess: refresh } })
  const markPosted = useMarkInvoicePosted({ mutation: { onSuccess: refresh } })
  const cancel = useCancelInvoice({
    mutation: {
      onSuccess: async () => {
        setCancelling(null)
        await refresh()
      },
    },
  })
  const bulkSubmit = useBulkSubmitInvoices({
    mutation: {
      onSuccess: async () => {
        setHandOverOpen(false)
        await refresh()
      },
    },
  })

  const pendingCount = pending.data?.length ?? 0

  const columns: DataListColumn<Invoice>[] = [
    {
      id: 'number',
      header: t('invoices.number'),
      primary: true,
      cell: (row) => <span className="numeric">{row.invoice_number}</span>,
    },
    { id: 'date', header: t('invoices.date'), cell: (row) => date(row.invoice_date) },
    {
      id: 'customer',
      header: t('nav.customers'),
      cell: (row) => row.customer_name || '—',
    },
    {
      id: 'patients',
      header: t('nav.patients'),
      desktopOnly: true,
      cell: (row) => row.patients.join(', '),
    },
    {
      id: 'status',
      header: t('invoices.status'),
      // Released and still here is the one status that is a task rather than a fact, so it is
      // the one that gets a colour.
      cell: (row) => (
        <span
          className={cn(
            'rounded-full px-2 py-0.5 text-xs',
            row.status === 'accepted' ? 'bg-rust-soft text-rust' : 'bg-sunken text-ink-soft',
          )}
          title={row.status === 'accepted' ? t('invoices.notSent') : undefined}
        >
          {t(`invoices.status_${row.status}`)}
        </span>
      ),
    },
    {
      id: 'total',
      header: t('treatments.total'),
      numeric: true,
      cell: (row) => money(row.total_gross),
    },
  ]

  const openTreatment = (row: Invoice) =>
    void navigate({ to: '/treatments/$id', params: { id: String(row.treatment_id) } })

  const rowActions = (row: Invoice) => (
    <>
      <a
        href={getInvoicePdfUrl(row.id)}
        target="_blank"
        rel="noreferrer"
        className="inline-flex min-h-11 items-center gap-1.5 rounded-control border border-line-strong bg-surface px-2.5 text-xs text-ink hover:bg-sunken sm:min-h-8"
      >
        <FileText className="size-3.5" />
        {t('invoices.pdf')}
      </a>
      {/* Nothing else can see a letter go into a postbox, and until it has, the invoice is
          not ready for the bookkeeper. */}
      {row.status === 'accepted' ? (
        <Button
          size="small"
          variant="primary"
          title={t('invoices.markPostedHint')}
          disabled={markPosted.isPending}
          onClick={() => markPosted.mutate({ id: row.id })}
        >
          <Send className="size-3.5" />
          {t('invoices.markPosted')}
        </Button>
      ) : null}
      {row.status === 'sent' ? (
        <Button
          size="small"
          variant="primary"
          disabled={submit.isPending}
          onClick={() => submit.mutate({ id: row.id })}
        >
          <PackageCheck className="size-3.5" />
          {t('invoices.submitShort')}
        </Button>
      ) : null}
      {row.status === 'cancelled' ? null : (
        <Button
          size="icon"
          variant="ghost"
          aria-label={t('invoices.cancel')}
          onClick={() => setCancelling(row)}
        >
          <Ban className="size-3.5 text-danger" />
        </Button>
      )}
    </>
  )

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader
        eyebrow={t('app.practice')}
        title={t('invoices.title')}
        actions={
          <>
            <a
              href={getPendingInvoicePdfsUrl()}
              aria-disabled={pendingCount === 0}
              className={
                pendingCount === 0
                  ? 'pointer-events-none inline-flex min-h-11 items-center gap-2 rounded-control border border-line px-4 text-sm text-ink-faint sm:min-h-9'
                  : 'inline-flex min-h-11 items-center gap-2 rounded-control border border-line-strong bg-surface px-4 text-sm text-ink hover:bg-sunken sm:min-h-9'
              }
            >
              <Download className="size-4" />
              {t('invoices.bulkDownload')}
            </a>
            <Button
              variant="accent"
              disabled={pendingCount === 0}
              onClick={() => setHandOverOpen(true)}
            >
              <PackageCheck className="size-4" />
              {t('invoices.bulkSubmit')}
            </Button>
          </>
        }
      />

      <section className="mt-5 rounded-card border border-line bg-cream-soft p-4">
        <p className="eyebrow">{t('invoices.pending')}</p>
        <p className="numeric mt-0.5 text-2xl font-bold text-rust">{pendingCount}</p>
        <DataList
          className="mt-3"
          data={pending.data ?? []}
          columns={columns}
          getRowId={(row) => String(row.id)}
          isLoading={pending.isPending}
          error={pending.isError ? t('list.error') : null}
          emptyMessage={t('invoices.nothingPending')}
          rowActions={rowActions}
          onRowClick={openTreatment}
        />
      </section>

      <section className="mt-8">
        <h2 className="text-base">{t('invoices.all')}</h2>
        <div className="mt-3 flex flex-wrap items-center gap-4">
          <input
            type="search"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('invoices.searchPlaceholder')}
            className="w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9 sm:max-w-sm"
          />
          <CheckboxField
            label={t('invoices.showCancelled')}
            checked={showCancelled}
            onChange={(event) => setShowCancelled(event.target.checked)}
          />
        </div>

        <DataList
          className="mt-3"
          data={invoices.data ?? []}
          columns={columns}
          getRowId={(row) => String(row.id)}
          isLoading={invoices.isPending}
          error={invoices.isError ? t('list.error') : null}
          rowActions={rowActions}
          onRowClick={openTreatment}
        />
      </section>

      <ConfirmDialog
        open={handOverOpen}
        onOpenChange={setHandOverOpen}
        title={t('invoices.bulkSubmit')}
        confirmLabel={t('invoices.bulkSubmit')}
        variant="primary"
        busy={bulkSubmit.isPending}
        onConfirm={() => bulkSubmit.mutate()}
      >
        {t('invoices.bulkSubmitConfirm', { count: pendingCount })}
      </ConfirmDialog>

      <ConfirmDialog
        open={cancelling !== null}
        onOpenChange={(open) => {
          if (!open) setCancelling(null)
        }}
        title={t('invoices.cancel')}
        description={cancelling?.invoice_number}
        busy={cancel.isPending}
        onConfirm={() => {
          if (cancelling) cancel.mutate({ id: cancelling.id })
        }}
      >
        {t('invoices.cancelConfirm')}
      </ConfirmDialog>
    </div>
  )
}
