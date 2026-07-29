import i18next from 'i18next'
import { initReactI18next } from 'react-i18next'
import de from '@/i18n/de.json'
import en from '@/i18n/en.json'
import { DEFAULT_LOCALE, type Locale } from '@/lib/format'

const STORAGE_KEY = 'kvet.locale'

export const LOCALES: Locale[] = ['de-DE', 'en-US']

function storedLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    if (stored && (LOCALES as string[]).includes(stored)) return stored as Locale
  } catch {
    // Private mode without storage: fall back to the default.
  }
  return DEFAULT_LOCALE
}

await i18next.use(initReactI18next).init({
  resources: {
    'de-DE': { translation: de },
    'en-US': { translation: en },
  },
  lng: storedLocale(),
  fallbackLng: DEFAULT_LOCALE,
  interpolation: { escapeValue: false },
})

/** Switches the UI language and remembers the choice. */
export function setLocale(locale: Locale): void {
  void i18next.changeLanguage(locale)
  document.documentElement.lang = locale
  try {
    window.localStorage.setItem(STORAGE_KEY, locale)
  } catch {
    // Not being able to remember the choice is not worth an error.
  }
}

/** The active locale, always one of the supported ones. */
export function currentLocale(): Locale {
  return (LOCALES as string[]).includes(i18next.language)
    ? (i18next.language as Locale)
    : DEFAULT_LOCALE
}

export default i18next
