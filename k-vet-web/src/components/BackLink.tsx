import { Link, useRouter } from '@tanstack/react-router'
import { ArrowLeft } from 'lucide-react'
import type { MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { usePreviousPage } from '@/lib/trail'

interface BackLinkProps {
  /**
   * Route to return to when the page was not reached from another page of the app — a link
   * opened in a new tab: the record's list, or its parent record where it has one.
   */
  to: string
  params?: Record<string, string> | undefined
  /** What `to` is, named: "Behandlungsgruppen", "Amoxicillin". */
  label: string
}

const CLASS_NAME = 'eyebrow inline-flex min-h-11 items-center gap-1 hover:text-ink-soft sm:min-h-0'

/**
 * The way out of a detail page: back to the page the vet came from, named — the animal an
 * appointment was opened from, not the day list (issues.md). Going back through the history
 * returns that page as it was, with its tab and its scroll. Reached from nowhere in this tab,
 * the record's own parent stands in.
 */
export function BackLink({ to, params, label }: BackLinkProps) {
  const { t } = useTranslation()
  const router = useRouter()
  const previous = usePreviousPage()

  if (previous) {
    const goBack = (event: MouseEvent<HTMLAnchorElement>) => {
      // A modified click opens the page in a new tab or window, as any link does.
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
        return
      }
      event.preventDefault()
      router.history.back()
    }
    return (
      <a
        href={previous.href}
        onClick={goBack}
        aria-label={`${t('action.back')}: ${previous.title}`}
        className={CLASS_NAME}
      >
        <ArrowLeft className="size-3.5" aria-hidden />
        {previous.title}
      </a>
    )
  }

  return (
    <Link
      to={to}
      params={params}
      aria-label={`${t('action.back')}: ${label}`}
      className={CLASS_NAME}
    >
      <ArrowLeft className="size-3.5" aria-hidden />
      {label}
    </Link>
  )
}
