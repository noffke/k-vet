import { ChevronDown, Trash2 } from 'lucide-react'
import type { ReactNode } from 'react'
import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * The positions of an appointment or a Preisauskunft as one ruled list that reads like the
 * invoice it becomes: the name on the left, quantity, factor and amount in columns that line up
 * down the list. On a laptop the list is the grid and every row takes its columns from it
 * (`subgrid`), so a column is as wide as its widest value — a long unit ("Tablette") widens its
 * column instead of spilling out of a fixed one.
 */
export function PositionList({ label, children }: { label: string; children: ReactNode }) {
  return (
    <ul
      aria-label={label}
      className={cn(
        'mt-2 flex flex-col divide-y divide-line rounded-card border border-line bg-surface',
        // icon · name · quantity · factor · amount · bin
        'sm:grid sm:grid-cols-[auto_minmax(0,1fr)_auto_auto_auto_auto]',
      )}
    >
      {children}
    </ul>
  )
}

interface PositionRowProps {
  icon: ReactNode
  name: string
  /** The quantity's cell — or a Wegegeld line's kilometres. */
  quantity: ReactNode
  /** A service's factor; a drug has none, and its column stays empty. */
  factor?: ReactNode
  amount: string
  /** Shown under the row while the quantity is being typed: the steps, what it bills as. */
  hint?: ReactNode
  /**
   * What must not wait behind the disclosure — a missing km, a changed price, a refusal, a line
   * changed since the PDF; the caller sets each line's tone.
   */
  notice?: ReactNode
  /** Everything else, behind the name. */
  details: ReactNode
  /** Changed since the invoice PDF: a rust rule along the left edge. */
  marker?: boolean
  /** The line just picked, its editors open. */
  newest?: boolean
  onRemove?: (() => void) | undefined
  removeLabel: string
}

/**
 * One position, collapsed to a line: icon, name, its editable values, its amount and the bin.
 * The name opens the rest. On a phone the values move under the name, the amount and the bin
 * staying on the first line beside it.
 */
export function PositionRow({
  icon,
  name,
  quantity,
  factor,
  amount,
  hint,
  notice,
  details,
  marker,
  newest,
  onRemove,
  removeLabel,
}: PositionRowProps) {
  const [expanded, setExpanded] = useState(false)
  const detailsId = useId()

  return (
    <li
      className={cn(
        'grid grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-x-2 px-3 py-1',
        'sm:col-span-full sm:grid-cols-subgrid',
        newest && 'bg-rust-soft/30',
        marker && 'shadow-[inset_4px_0_0_var(--color-rust)]',
      )}
    >
      <span className="col-start-1 row-start-1 flex">{icon}</span>
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={detailsId}
        onClick={() => setExpanded((open) => !open)}
        className={cn(
          'group col-start-2 row-start-1 flex min-h-11 min-w-0 items-center py-1 text-left sm:min-h-9',
          'font-medium text-ink [overflow-wrap:anywhere] hover:text-rust',
        )}
      >
        <span className="min-w-0">
          {name}
          <ChevronDown
            className="ml-1 inline size-4 align-[-0.15em] text-ink-faint transition-transform group-aria-expanded:rotate-180"
            aria-hidden
          />
        </span>
      </button>
      {/* On a phone a line of their own under the name; on a laptop each in its column. */}
      <div className="col-span-3 col-start-2 row-start-2 flex flex-wrap items-center gap-x-3 gap-y-1 pb-1 sm:contents">
        <div className="min-w-0 sm:col-start-3 sm:row-start-1 sm:pb-0">{quantity}</div>
        {factor ? <div className="min-w-0 sm:col-start-4 sm:row-start-1">{factor}</div> : null}
      </div>
      <span className="numeric col-start-3 row-start-1 whitespace-nowrap text-right font-semibold text-ink sm:col-start-5 sm:pl-2">
        {amount}
      </span>
      <span className="col-start-4 row-start-1 flex sm:col-start-6">
        {onRemove ? (
          <Button size="icon" variant="ghost" aria-label={removeLabel} onClick={onRemove}>
            <Trash2 className="size-4 text-danger" />
          </Button>
        ) : null}
      </span>
      {hint ? (
        <div className="col-span-3 col-start-2 pb-1 text-xs text-ink-faint sm:col-span-4 sm:text-right">
          {hint}
        </div>
      ) : null}
      {notice ? (
        <div className="col-span-3 col-start-2 flex flex-col pb-1.5 text-xs sm:col-span-4">
          {notice}
        </div>
      ) : null}
      <div id={detailsId} hidden={!expanded} className="col-span-full">
        {expanded ? (
          <div className="mb-2 rounded-control bg-sunken/70 p-3 text-sm">{details}</div>
        ) : null}
      </div>
    </li>
  )
}
