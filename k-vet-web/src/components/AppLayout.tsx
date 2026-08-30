import * as Dialog from '@radix-ui/react-dialog'
import { Link, Outlet, useRouterState } from '@tanstack/react-router'
import {
  Building2,
  CalendarDays,
  FileText,
  LayoutDashboard,
  LogOut,
  MoreHorizontal,
  NotebookPen,
  Package,
  ReceiptText,
  Settings,
  Stethoscope,
  Users,
} from 'lucide-react'
import type { ComponentType } from 'react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLogout } from '@/features/auth/session'
import type { Locale } from '@/lib/format'
import { LOCALES, setLocale } from '@/lib/i18n'
import { useLocaleFormat } from '@/lib/locale'
import { cn } from '@/lib/utils'

interface NavItem {
  to: string
  labelKey: string
  icon: ComponentType<{ className?: string }>
  /** Reachable from the phone tab bar without opening the overflow sheet. */
  onTabBar?: boolean
}

const NAV: NavItem[] = [
  { to: '/', labelKey: 'nav.dashboard', icon: LayoutDashboard, onTabBar: true },
  { to: '/appointments', labelKey: 'nav.appointments', icon: CalendarDays, onTabBar: true },
  { to: '/customers', labelKey: 'nav.customers', icon: Users, onTabBar: true },
  { to: '/pharmacy', labelKey: 'nav.pharmacy', icon: Package, onTabBar: true },
  { to: '/services', labelKey: 'nav.services', icon: Stethoscope },
  { to: '/templates', labelKey: 'nav.templates', icon: FileText },
  { to: '/text-blocks', labelKey: 'nav.textBlocks', icon: NotebookPen },
  { to: '/masterdata', labelKey: 'nav.masterData', icon: Building2 },
  { to: '/invoices', labelKey: 'nav.invoices', icon: ReceiptText, onTabBar: true },
  { to: '/settings', labelKey: 'nav.settings', icon: Settings },
]

/**
 * Application chrome. On laptop and desktop a fixed rail on the left; on a phone a
 * bottom tab bar, because during a house call the vet holds an animal in one hand and
 * the phone in the other — the primary sections have to be in thumb reach.
 */
export function AppLayout() {
  const { t } = useTranslation()
  const signOut = useLogout()
  const { locale } = useLocaleFormat()
  const [overflowOpen, setOverflowOpen] = useState(false)
  const path = useRouterState({ select: (state) => state.location.pathname })

  const isActive = (to: string) => (to === '/' ? path === '/' : path.startsWith(to))
  const tabItems = NAV.filter((item) => item.onTabBar)

  return (
    <div className="min-h-dvh sm:flex">
      {/* Desktop rail */}
      <nav className="hidden w-56 shrink-0 flex-col border-r border-line bg-cream px-3 py-4 sm:flex">
        <Link to="/" className="px-2">
          <span className="block text-[0.6875rem] font-bold uppercase tracking-[0.18em] text-ink-soft">
            {t('app.practice')}
          </span>
          <span className="block text-2xl font-bold uppercase tracking-tight text-rust">k-vet</span>
        </Link>

        <ul className="mt-6 flex flex-1 flex-col gap-0.5">
          {NAV.map((item) => (
            <li key={item.to}>
              <Link
                to={item.to}
                className={cn(
                  'flex items-center gap-2.5 rounded-control px-2.5 py-2 text-sm',
                  isActive(item.to)
                    ? 'bg-surface font-semibold text-rust'
                    : 'text-ink hover:bg-cream-soft',
                )}
              >
                <item.icon className="size-4 shrink-0" />
                {t(item.labelKey)}
              </Link>
            </li>
          ))}
        </ul>

        <div className="mt-4 flex items-center justify-between gap-2 border-t border-line-strong pt-3">
          <LanguageSwitch active={locale} />
          <button
            type="button"
            onClick={() => signOut.mutate()}
            className="flex items-center gap-1.5 text-xs text-ink-soft hover:text-rust"
          >
            <LogOut className="size-3.5" />
            {t('app.logout')}
          </button>
        </div>
      </nav>

      {/* Phone header */}
      <header className="flex items-center justify-between border-b border-line bg-cream px-4 py-2.5 sm:hidden">
        <Link to="/" className="text-lg font-bold uppercase tracking-tight text-rust">
          k-vet
        </Link>
        <LanguageSwitch active={locale} />
      </header>

      <main className="min-w-0 flex-1 px-4 pt-4 pb-24 sm:px-6 sm:py-6 sm:pb-6">
        <Outlet />
      </main>

      {/* Phone tab bar */}
      <nav className="fixed inset-x-0 bottom-0 z-20 flex border-t border-line bg-cream pb-[env(safe-area-inset-bottom)] sm:hidden">
        {tabItems.map((item) => (
          <Link
            key={item.to}
            to={item.to}
            className={cn(
              'flex flex-1 flex-col items-center gap-0.5 py-2 text-[0.625rem]',
              isActive(item.to) ? 'font-semibold text-rust' : 'text-ink-soft',
            )}
          >
            <item.icon className="size-5" />
            {t(item.labelKey)}
          </Link>
        ))}
        <Dialog.Root open={overflowOpen} onOpenChange={setOverflowOpen}>
          <Dialog.Trigger className="flex flex-1 flex-col items-center gap-0.5 py-2 text-[0.625rem] text-ink-soft">
            <MoreHorizontal className="size-5" />
            {t('action.open')}
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="fixed inset-0 z-30 bg-ink/30" />
            <Dialog.Content className="fixed inset-x-0 bottom-0 z-40 rounded-t-card border-t border-line bg-surface p-4 pb-[calc(1rem+env(safe-area-inset-bottom))]">
              <Dialog.Title className="eyebrow">{t('app.name')}</Dialog.Title>
              <ul className="mt-3 flex flex-col">
                {NAV.filter((item) => !item.onTabBar).map((item) => (
                  <li key={item.to}>
                    <Link
                      to={item.to}
                      onClick={() => setOverflowOpen(false)}
                      className="flex items-center gap-3 py-3 text-sm text-ink"
                    >
                      <item.icon className="size-4" />
                      {t(item.labelKey)}
                    </Link>
                  </li>
                ))}
                <li>
                  <button
                    type="button"
                    onClick={() => signOut.mutate()}
                    className="flex w-full items-center gap-3 py-3 text-sm text-ink-soft"
                  >
                    <LogOut className="size-4" />
                    {t('app.logout')}
                  </button>
                </li>
              </ul>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      </nav>
    </div>
  )
}

function LanguageSwitch({ active }: { active: Locale }) {
  const { t } = useTranslation()
  return (
    <fieldset className="flex items-center gap-1 border-0 p-0">
      <legend className="sr-only">{t('app.language')}</legend>
      {LOCALES.map((locale) => (
        <button
          key={locale}
          type="button"
          onClick={() => setLocale(locale)}
          aria-pressed={locale === active}
          className={cn(
            'rounded px-1.5 py-0.5 text-[0.6875rem] font-bold uppercase tracking-wider',
            locale === active ? 'bg-surface text-rust' : 'text-ink-faint hover:text-ink',
          )}
        >
          {locale.slice(0, 2)}
        </button>
      ))}
    </fieldset>
  )
}
