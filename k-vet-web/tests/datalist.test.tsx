import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { DataList, type DataListColumn } from '@/components/DataList'

interface Row {
  id: number
  name: string
  amount: string
}

const rows: Row[] = [
  { id: 1, name: 'Bello', amount: '12.50' },
  { id: 2, name: 'Minka', amount: '3.00' },
]

const columns: DataListColumn<Row>[] = [
  { id: 'name', header: 'Name', cell: (row) => row.name, primary: true },
  { id: 'amount', header: 'Betrag', cell: (row) => row.amount, numeric: true },
]

describe('DataList', () => {
  it('renders a table for desktop and cards for phones from one column definition', () => {
    render(<DataList data={rows} columns={columns} getRowId={(row) => String(row.id)} />)

    // Desktop: a real table with column headers.
    expect(screen.getByRole('columnheader', { name: 'Name' })).toBeInTheDocument()
    expect(screen.getAllByRole('row')).toHaveLength(3)

    // Phone: the same rows as a list of cards (both layouts are rendered, CSS picks one).
    const cards = screen.getByRole('list')
    expect(cards).toBeInTheDocument()
    expect(screen.getAllByText('Bello')).toHaveLength(2)
  })

  it('calls back with the clicked row', async () => {
    const onRowClick = vi.fn()
    render(
      <DataList
        data={rows}
        columns={columns}
        getRowId={(row) => String(row.id)}
        onRowClick={onRowClick}
      />,
    )

    await userEvent.click(screen.getAllByText('Minka')[0] as HTMLElement)

    expect(onRowClick).toHaveBeenCalledWith(rows[1])
  })

  it('renders a sub-line under the row in both layouts', async () => {
    const onRowClick = vi.fn()
    render(
      <DataList
        data={rows}
        columns={columns}
        getRowId={(row) => String(row.id)}
        onRowClick={onRowClick}
        rowSubline={(row) => (row.id === 1 ? `Musterweg 5 · 12345 Musterstadt` : null)}
      />,
    )

    // Once in the table's own `<tr>`, once inside the phone card.
    expect(screen.getAllByText('Musterweg 5 · 12345 Musterstadt')).toHaveLength(2)
    // The sub-line spans every column rather than sitting inside one.
    const cell = screen.getAllByText('Musterweg 5 · 12345 Musterstadt')[0] as HTMLElement
    expect(cell.closest('td')).toHaveAttribute('colspan', '2')
    // Rows without one are unaffected.
    expect(screen.getAllByText('Minka')).toHaveLength(2)

    // Clicking the sub-line opens the row it belongs to, like the cells above it.
    await userEvent.click(cell)
    expect(onRowClick).toHaveBeenCalledWith(rows[0])
  })

  it('shows an empty state instead of an empty table', () => {
    render(
      <DataList
        data={[]}
        columns={columns}
        getRowId={(row) => String(row.id)}
        emptyMessage="Keine Einträge"
      />,
    )

    expect(screen.getByText('Keine Einträge')).toBeInTheDocument()
    expect(screen.queryByRole('table')).not.toBeInTheDocument()
  })

  it('reports a load error', () => {
    render(
      <DataList
        data={[]}
        columns={columns}
        getRowId={(row) => String(row.id)}
        error="Konnte nicht geladen werden"
      />,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Konnte nicht geladen werden')
  })
})
