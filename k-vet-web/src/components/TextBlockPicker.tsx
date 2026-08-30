import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useListTextBlocks } from '@/api/generated/endpoints'
import type { TextBlock } from '@/api/generated/model'
import { Dialog } from '@/components/ui/dialog'

/** Keystrokes settle before a request goes out — same as the position picker. */
const SEARCH_DEBOUNCE_MS = 200

interface TextBlockPickerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onPick: (block: TextBlock) => void
}

/**
 * Choosing a Textbaustein to drop into a Vorbericht or a Therapie (issues.md 11).
 *
 * Search covers the content as well as the name: the vet remembers the wording of a block long
 * before they remember what they called it. Each entry shows the beginning of its text, because
 * a list of names alone does not say which of three vaccination blocks this is.
 */
export function TextBlockPicker({ open, onOpenChange, onPick }: TextBlockPickerProps) {
  const { t } = useTranslation()
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(term), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [term])

  const blocks = useListTextBlocks(
    { q: debounced || undefined },
    { query: { enabled: open, placeholderData: (previous) => previous } },
  )
  // An unfinished block has no text to insert.
  const entries = (blocks.data ?? []).filter((block) => !block.draft)

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('textBlocks.pickerTitle')}>
      <input
        type="search"
        // The dialog opens on a click and the search box is the whole point of it, so nothing
        // else on screen is competing for focus.
        autoFocus
        value={term}
        onChange={(event) => setTerm(event.target.value)}
        placeholder={t('textBlocks.searchPlaceholder')}
        className="w-full rounded-control border border-line-strong bg-surface px-3 py-2 min-h-11 sm:min-h-9"
      />

      <ul className="mt-3 flex flex-col gap-2">
        {entries.map((block) => (
          <li key={block.id}>
            <button
              type="button"
              onClick={() => onPick(block)}
              className="w-full rounded-card border border-line bg-surface px-3 py-2 text-left hover:bg-cream-soft"
            >
              <span className="block text-sm font-medium text-ink">{block.name}</span>
              <span className="mt-0.5 line-clamp-2 block text-xs text-ink-faint">
                {block.content}
              </span>
            </button>
          </li>
        ))}
        {entries.length === 0 ? (
          <li className="rounded-card border border-dashed border-line-strong px-4 py-6 text-center text-sm text-ink-faint">
            {t('textBlocks.empty')}
          </li>
        ) : null}
      </ul>
    </Dialog>
  )
}
