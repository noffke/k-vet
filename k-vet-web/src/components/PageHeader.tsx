import type { ReactNode } from 'react'

interface PageHeaderProps {
  /** The way back, on a detail page — a `BackLink`, rendered in place of the eyebrow. */
  back?: ReactNode
  /** Small caps line above the title; may be a link to a related record. */
  eyebrow?: ReactNode
  title: string
  /** Save indicator, primary action, status badges. */
  actions?: ReactNode
  children?: ReactNode
}

/** The shared page opening: small caps eyebrow, terracotta title, actions on the right. */
export function PageHeader({ back, eyebrow, title, actions, children }: PageHeaderProps) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-x-4 gap-y-2 border-b border-line pb-3">
      <div className="min-w-0">
        {back}
        {eyebrow ? <div className="eyebrow">{eyebrow}</div> : null}
        <h1 className="truncate text-xl sm:text-2xl">{title}</h1>
        {children}
      </div>
      {/* Wrapping keeps a header with several actions inside a phone's width. */}
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </header>
  )
}
