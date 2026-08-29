import { QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { Toaster, toast } from 'sonner'
import { setUnauthorizedHandler } from '@/api/fetcher'
import './index.css'
// Initialises i18next (de-DE default) before the first render.
import i18n from '@/lib/i18n'
import { queryClient } from '@/lib/query'
import { router } from '@/router'

const container = document.getElementById('root')
if (!container) throw new Error('missing #root element')

/**
 * A session that ended while the tab sat open (FR-001a): say so once, drop every cached
 * answer so nothing from the old session survives a re-login, and go back to the sign-in
 * screen. Several requests usually fail together, so this runs only for the first of them.
 */
let handlingExpiry = false
setUnauthorizedHandler(() => {
  if (handlingExpiry) return
  handlingExpiry = true

  toast.warning(i18n.t('login.expired'))
  queryClient.clear()
  void router.navigate({ to: '/login', search: { expired: true }, replace: true }).finally(() => {
    handlingExpiry = false
  })
})

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      {/* The app's only toast. Sonner ships a shadowed, self-styled card; these classes
          put it on the same paper, rules and rust as everything else. */}
      <Toaster
        position="top-center"
        toastOptions={{
          classNames: {
            toast: 'rounded-card border border-line bg-cream-soft text-ink',
            title: 'text-sm font-medium text-rust',
          },
        }}
      />
    </QueryClientProvider>
  </StrictMode>,
)
