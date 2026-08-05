import { Link } from '@tanstack/react-router'
import { ArrowLeft } from 'lucide-react'
import { useTranslation } from 'react-i18next'

interface BackLinkProps {
  /** Route to return to — the record's list, or its parent record where it has one. */
  to: string
  params?: Record<string, string> | undefined
  /** What is being returned to, named: "Behandlungsgruppen", "Amoxicillin". */
  label: string
}

/**
 * The way out of a detail page. Every record is opened from somewhere, and the nav rail only
 * says which section you are in — this says which list or record you came from, and goes there.
 */
export function BackLink({ to, params, label }: BackLinkProps) {
  const { t } = useTranslation()
  return (
    <Link
      to={to}
      params={params}
      aria-label={`${t('action.back')}: ${label}`}
      className="eyebrow inline-flex min-h-11 items-center gap-1 hover:text-ink-soft sm:min-h-0"
    >
      <ArrowLeft className="size-3.5" aria-hidden />
      {label}
    </Link>
  )
}
