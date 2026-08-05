import { Command } from 'cmdk'
import { FileText, Pill, Stethoscope } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useListTemplates, usePickerItems } from '@/api/generated/endpoints'
import type { PickerItem, Template } from '@/api/generated/model'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

/** Keystrokes settle before a request goes out — the picker is typed into fast. */
const SEARCH_DEBOUNCE_MS = 200

/**
 * Composes the entry's display label. Quantities are formatted in the active locale, so
 * the label is built here rather than on the server.
 */
function pickerLabel(
  item: PickerItem,
  formatQuantity: (value: string | null | undefined) => string,
): string {
  if (item.kind === 'drug_packaging') {
    const size = item.quantity ? `${formatQuantity(item.quantity)} ${item.unit ?? ''}`.trim() : ''
    return size ? `${item.name} · ${size}` : item.name
  }
  return item.got_number ? `${item.got_number} · ${item.name}` : item.name
}

interface ItemPickerProps {
  onPick: (item: PickerItem) => void
  /**
   * When given, Behandlungsgruppen are searched too and listed in their own group above
   * the positions. Left out where a group would make no sense — inside a group itself.
   */
  onPickTemplate?: ((template: Template) => void) | undefined
  disabled?: boolean
  placeholder?: string
}

/**
 * The unified drug/service picker (FR-005): one search box, results ranked by how often
 * the practice used them, drugs and services told apart by icon. This is the highest
 * frequency interaction in the app, so it stays open after a pick.
 *
 * Behandlungsgruppen are searched alongside them but kept in a group of their own: they
 * are not billable positions, they insert several, and a group has no single price or VAT
 * rate to rank or show next to one.
 */
export function ItemPicker({ onPick, onPickTemplate, disabled, placeholder }: ItemPickerProps) {
  const { t } = useTranslation()
  const { money, quantity } = useLocaleFormat()
  const label = (item: PickerItem) => pickerLabel(item, quantity)
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(term), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [term])

  const results = usePickerItems(
    { q: debounced || undefined },
    { query: { enabled: !disabled, placeholderData: (previous) => previous } },
  )
  const templates = useListTemplates(
    { q: debounced || undefined },
    {
      query: {
        enabled: !disabled && onPickTemplate !== undefined,
        placeholderData: (previous) => previous,
      },
    },
  )
  // An unfinished group would apply nothing, so it is not offered.
  const groups = onPickTemplate ? (templates.data ?? []).filter((template) => !template.draft) : []

  return (
    <Command
      // Ranking comes from the server (usage weights); the list must not reshuffle it.
      shouldFilter={false}
      className="rounded-card border border-line-strong bg-surface"
    >
      <Command.Input
        value={term}
        onValueChange={setTerm}
        disabled={disabled}
        placeholder={
          placeholder ??
          (onPickTemplate ? t('treatments.pickerPlaceholder') : t('treatments.pickerItemsOnly'))
        }
        className="w-full rounded-t-card border-b border-line bg-transparent px-3 py-2.5 text-ink placeholder:text-ink-faint focus:outline-none"
      />
      <Command.List className="max-h-64 overflow-y-auto p-1">
        {results.isPending ? (
          <Command.Loading>
            <p className="px-2 py-3 text-sm text-ink-faint">{t('list.loading')}</p>
          </Command.Loading>
        ) : null}
        <Command.Empty className="px-2 py-3 text-sm text-ink-faint">
          {t('list.empty')}
        </Command.Empty>

        {groups.length > 0 && onPickTemplate ? (
          <Command.Group
            heading={t('templates.title')}
            className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5"
          >
            {groups.map((template) => (
              <Command.Item
                key={`template-${template.id}`}
                value={`template-${template.id}`}
                onSelect={() => {
                  onPickTemplate(template)
                  setTerm('')
                }}
                className={cn(
                  'flex cursor-pointer items-center gap-2.5 rounded-control px-2 py-2 text-sm',
                  'data-[selected=true]:bg-cream-soft',
                )}
              >
                <FileText
                  className="size-4 shrink-0 text-sage-deep"
                  aria-label={t('templates.title')}
                />
                <span className="min-w-0 flex-1 truncate text-ink">{template.name}</span>
                <span className="numeric shrink-0 text-xs text-ink-faint">
                  {t('templates.itemCount', { count: template.item_count })}
                </span>
              </Command.Item>
            ))}
          </Command.Group>
        ) : null}

        <Command.Group
          heading={groups.length > 0 ? t('treatments.items') : undefined}
          className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5"
        >
          {(results.data ?? []).map((item) => (
            <Command.Item
              key={`${item.kind}-${item.id}`}
              value={`${item.kind}-${item.id}`}
              onSelect={() => {
                onPick(item)
                setTerm('')
              }}
              className={cn(
                'flex cursor-pointer items-center gap-2.5 rounded-control px-2 py-2 text-sm',
                'data-[selected=true]:bg-cream-soft',
              )}
            >
              {item.kind === 'drug_packaging' ? (
                <Pill className="size-4 shrink-0 text-rust" aria-label={t('pharmacy.drugs')} />
              ) : (
                <Stethoscope
                  className="size-4 shrink-0 text-ink-soft"
                  aria-label={t('services.title')}
                />
              )}
              <span className="min-w-0 flex-1 truncate text-ink">{label(item)}</span>
              {item.kind === 'drug_packaging' ? (
                <span className="numeric shrink-0 text-xs text-ink-faint">
                  {quantity(item.in_stock)} {item.unit ?? ''}
                </span>
              ) : null}
              <span className="numeric shrink-0 text-xs text-ink-soft">
                {money(item.price_gross)}
              </span>
            </Command.Item>
          ))}
        </Command.Group>
      </Command.List>
    </Command>
  )
}
