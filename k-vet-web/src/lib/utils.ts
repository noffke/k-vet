import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

/** Merges conditional class names, letting later Tailwind utilities win. */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs))
}

/**
 * `home_zip` → `homeZip`: turns a wire field name into the `field.*` i18n key that labels it.
 */
export function toCamelCase(field: string): string {
  return field.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
}
