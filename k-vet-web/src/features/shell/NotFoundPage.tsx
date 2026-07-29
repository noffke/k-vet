import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

export function NotFoundPage() {
  const { t } = useTranslation()
  return (
    <div className="mx-auto max-w-md px-4 py-16 text-center">
      <p className="eyebrow">404</p>
      <h1 className="mt-2 text-xl">{t('error.notFound')}</h1>
      <Link to="/" className="mt-4 inline-block text-sm text-rust underline underline-offset-4">
        {t('nav.dashboard')}
      </Link>
    </div>
  )
}
