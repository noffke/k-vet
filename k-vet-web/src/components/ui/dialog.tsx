import * as Primitive from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

interface DialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description?: string
  children: ReactNode
  /** Buttons; laid out right-aligned on desktop, stacked on a phone. */
  footer?: ReactNode
  className?: string
}

/**
 * The one modal in the app: centred on desktop, a bottom sheet on phones so the
 * confirm button lands in thumb reach.
 */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  className,
}: DialogProps) {
  const { t } = useTranslation()
  return (
    <Primitive.Root open={open} onOpenChange={onOpenChange}>
      <Primitive.Portal>
        <Primitive.Overlay className="fixed inset-0 z-30 bg-ink/30" />
        <Primitive.Content
          className={cn(
            'fixed z-40 flex max-h-[90dvh] flex-col overflow-hidden border border-line bg-surface',
            'inset-x-0 bottom-0 rounded-t-card pb-[env(safe-area-inset-bottom)]',
            'sm:inset-x-auto sm:bottom-auto sm:top-1/2 sm:left-1/2 sm:w-[32rem]',
            'sm:-translate-x-1/2 sm:-translate-y-1/2 sm:rounded-card',
            className,
          )}
        >
          <div className="flex items-start justify-between gap-4 border-b border-line px-4 py-3">
            <div>
              <Primitive.Title className="text-base font-semibold text-rust">
                {title}
              </Primitive.Title>
              {description ? (
                <Primitive.Description className="mt-0.5 text-sm text-ink-soft">
                  {description}
                </Primitive.Description>
              ) : null}
            </div>
            <Primitive.Close
              aria-label={t('action.closeDialog')}
              className="rounded-control p-1 text-ink-faint hover:bg-sunken hover:text-ink"
            >
              <X className="size-4" />
            </Primitive.Close>
          </div>

          <div className="flex-1 overflow-y-auto px-4 py-4">{children}</div>

          {footer ? (
            <div className="flex flex-col-reverse gap-2 border-t border-line px-4 py-3 sm:flex-row sm:justify-end">
              {footer}
            </div>
          ) : null}
        </Primitive.Content>
      </Primitive.Portal>
    </Primitive.Root>
  )
}
