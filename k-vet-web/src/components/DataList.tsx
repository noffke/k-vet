import { type ColumnDef, flexRender, getCoreRowModel, useReactTable } from '@tanstack/react-table'
import type { MouseEvent, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

export interface DataListColumn<T> {
  id: string
  header: string
  cell: (row: T) => ReactNode
  /** Right-aligned, tabular figures — for money and quantities. */
  numeric?: boolean
  /** The identifying column; on phones it becomes the card's headline. */
  primary?: boolean
  /** Dropped from the phone layout to keep cards scannable. */
  desktopOnly?: boolean
}

export interface DataListProps<T> {
  data: T[]
  columns: DataListColumn<T>[]
  getRowId: (row: T) => string
  onRowClick?: ((row: T) => void) | undefined
  /** Rendered under the headline on phones and in a leading cell on desktop. */
  rowBadge?: ((row: T) => ReactNode) | undefined
  /**
   * Per-row buttons and links. They sit in a trailing column on desktop and in a footer
   * strip on phones — never inside the card button, which may not nest interactive
   * elements.
   */
  rowActions?: ((row: T) => ReactNode) | undefined
  isLoading?: boolean | undefined
  error?: string | null | undefined
  emptyMessage?: string | undefined
  className?: string | undefined
}

/**
 * One list component for every screen: a real table on laptop and desktop, stacked cards
 * on a phone (Constitution III — both are first-class targets). The headless table keeps
 * a single column definition driving both layouts.
 */
export function DataList<T>({
  data,
  columns,
  getRowId,
  onRowClick,
  rowBadge,
  rowActions,
  isLoading,
  error,
  emptyMessage,
  className,
}: DataListProps<T>) {
  const { t } = useTranslation()

  const tableColumns: ColumnDef<T>[] = columns.map((column) => ({
    id: column.id,
    header: column.header,
    cell: (context) => column.cell(context.row.original),
  }))

  const table = useReactTable({
    data,
    columns: tableColumns,
    getCoreRowModel: getCoreRowModel(),
    getRowId,
  })

  if (isLoading) {
    return <p className="px-1 py-6 text-sm text-ink-faint">{t('list.loading')}</p>
  }
  if (error) {
    return (
      <p className="px-1 py-6 text-sm text-danger" role="alert">
        {error}
      </p>
    )
  }
  if (data.length === 0) {
    return (
      <p className="rounded-card border border-dashed border-line-strong px-4 py-8 text-center text-sm text-ink-faint">
        {emptyMessage ?? t('list.empty')}
      </p>
    )
  }

  /** A click on a row action is about that action, not about opening the row. */
  const handleRowClick = (row: T) => (event: MouseEvent<HTMLTableRowElement>) => {
    const target = event.target
    if (target instanceof Element && target.closest('a, button, input, select, label')) return
    onRowClick?.(row)
  }

  const primary = columns.find((column) => column.primary) ?? columns[0]
  const mobileColumns = columns.filter((column) => !column.desktopOnly && column.id !== primary?.id)

  return (
    <div className={className}>
      {/* Desktop: table */}
      <table className="hidden w-full border-separate border-spacing-0 sm:table">
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header, index) => {
                const column = columns[index]
                return (
                  <th
                    key={header.id}
                    scope="col"
                    className={cn(
                      'eyebrow border-b border-line bg-sunken px-3 py-2 text-left',
                      column?.numeric && 'text-right',
                    )}
                  >
                    {flexRender(header.column.columnDef.header, header.getContext())}
                  </th>
                )
              })}
              {rowActions ? (
                <th scope="col" className="border-b border-line bg-sunken px-3 py-2">
                  <span className="sr-only">{t('action.open')}</span>
                </th>
              ) : null}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr
              key={row.id}
              onClick={onRowClick ? handleRowClick(row.original) : undefined}
              className={cn('bg-surface', onRowClick && 'cursor-pointer hover:bg-cream-soft')}
            >
              {row.getVisibleCells().map((cell, index) => {
                const column = columns[index]
                return (
                  <td
                    key={cell.id}
                    className={cn(
                      'border-b border-line px-3 py-2 align-top',
                      column?.numeric && 'numeric text-right',
                      column?.primary && 'font-medium',
                    )}
                  >
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    {column?.primary && rowBadge ? (
                      <span className="ml-2">{rowBadge(row.original)}</span>
                    ) : null}
                  </td>
                )
              })}
              {rowActions ? (
                <td className="border-b border-line px-3 py-2 align-top text-right">
                  <span className="inline-flex flex-wrap justify-end gap-1">
                    {rowActions(row.original)}
                  </span>
                </td>
              ) : null}
            </tr>
          ))}
        </tbody>
      </table>

      {/* Phone: cards */}
      <ul className="flex flex-col gap-2 sm:hidden">
        {data.map((row) => (
          <li key={getRowId(row)}>
            <button
              type="button"
              onClick={onRowClick ? () => onRowClick(row) : undefined}
              disabled={!onRowClick}
              className={cn(
                'w-full rounded-card border border-line bg-surface px-3 py-3 text-left',
                onRowClick && 'active:bg-cream-soft',
                rowActions && 'rounded-b-none border-b-0',
              )}
            >
              <span className="flex items-center justify-between gap-2">
                <span className="font-medium text-ink">{primary?.cell(row)}</span>
                {rowBadge ? rowBadge(row) : null}
              </span>
              <dl className="mt-1.5 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
                {mobileColumns.map((column) => (
                  <div key={column.id} className="col-span-2 flex justify-between gap-3">
                    <dt className="eyebrow self-center">{column.header}</dt>
                    <dd
                      className={cn(
                        'text-sm text-ink-soft',
                        column.numeric && 'numeric text-right',
                      )}
                    >
                      {column.cell(row)}
                    </dd>
                  </div>
                ))}
              </dl>
            </button>
            {rowActions ? (
              <div className="flex flex-wrap gap-1 rounded-b-card border border-line bg-surface px-3 py-2">
                {rowActions(row)}
              </div>
            ) : null}
          </li>
        ))}
      </ul>
    </div>
  )
}
