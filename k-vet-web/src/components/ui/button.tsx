import { cva, type VariantProps } from 'class-variance-authority'
import type { ButtonHTMLAttributes } from 'react'
import { cn } from '@/lib/utils'

const button = cva(
  'inline-flex items-center justify-center gap-2 rounded-control font-medium transition-colors ' +
    'disabled:pointer-events-none disabled:opacity-50 whitespace-nowrap',
  {
    variants: {
      variant: {
        primary: 'bg-ink text-cream-soft hover:bg-ink/90',
        accent: 'bg-rust text-white hover:bg-rust-bright',
        secondary: 'border border-line-strong bg-surface text-ink hover:bg-sunken',
        ghost: 'text-ink-soft hover:bg-sunken hover:text-ink',
        danger: 'border border-danger/30 bg-surface text-danger hover:bg-danger/10',
      },
      size: {
        // Touch targets stay at 44px on phones (SC-009).
        default: 'min-h-11 px-4 text-sm sm:min-h-9',
        small: 'min-h-9 px-3 text-sm',
        icon: 'size-11 sm:size-9',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'default' },
  },
)

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & VariantProps<typeof button>

export function Button({ className, variant, size, type = 'button', ...props }: ButtonProps) {
  return <button type={type} className={cn(button({ variant, size }), className)} {...props} />
}
