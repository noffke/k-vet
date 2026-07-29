import type {
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from 'react'
import { useId } from 'react'
import { cn } from '@/lib/utils'

const controlClasses =
  'w-full rounded-control border border-line-strong bg-surface px-3 py-2 text-ink ' +
  'placeholder:text-ink-faint min-h-11 sm:min-h-9 disabled:bg-sunken disabled:text-ink-faint'

const invalidClasses = 'border-danger bg-danger/5'

interface FieldShellProps {
  label: string
  error?: string | undefined
  hint?: string | undefined
  className?: string | undefined
  children: (id: string) => ReactNode
}

/** Label, control and field-level error — the one shape every form field uses. */
export function Field({ label, error, hint, className, children }: FieldShellProps) {
  const id = useId()
  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <label htmlFor={id} className="text-xs font-semibold text-ink-soft">
        {label}
      </label>
      {children(id)}
      {error ? (
        <p className="text-xs font-medium text-danger" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="text-xs text-ink-faint">{hint}</p>
      ) : null}
    </div>
  )
}

export type TextFieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'id'> & {
  label: string
  error?: string | undefined
  hint?: string | undefined
  wrapperClassName?: string | undefined
}

export function TextField({
  label,
  error,
  hint,
  className,
  wrapperClassName,
  ...props
}: TextFieldProps) {
  return (
    <Field label={label} error={error} hint={hint} className={wrapperClassName}>
      {(id) => (
        <input
          id={id}
          aria-invalid={error ? true : undefined}
          className={cn(controlClasses, error && invalidClasses, className)}
          {...props}
        />
      )}
    </Field>
  )
}

export type TextAreaFieldProps = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'id'> & {
  label: string
  error?: string | undefined
  hint?: string | undefined
  wrapperClassName?: string | undefined
}

export function TextAreaField({
  label,
  error,
  hint,
  className,
  wrapperClassName,
  ...props
}: TextAreaFieldProps) {
  return (
    <Field label={label} error={error} hint={hint} className={wrapperClassName}>
      {(id) => (
        <textarea
          id={id}
          rows={props.rows ?? 3}
          aria-invalid={error ? true : undefined}
          className={cn(controlClasses, 'min-h-20 resize-y', error && invalidClasses, className)}
          {...props}
        />
      )}
    </Field>
  )
}

export type SelectFieldProps = Omit<SelectHTMLAttributes<HTMLSelectElement>, 'id'> & {
  label: string
  error?: string | undefined
  hint?: string | undefined
  wrapperClassName?: string | undefined
}

/**
 * A native select: on a phone it opens the platform picker, which beats every
 * custom dropdown for one-handed use during a house call.
 */
export function SelectField({
  label,
  error,
  hint,
  className,
  wrapperClassName,
  children,
  ...props
}: SelectFieldProps) {
  return (
    <Field label={label} error={error} hint={hint} className={wrapperClassName}>
      {(id) => (
        <select
          id={id}
          aria-invalid={error ? true : undefined}
          className={cn(controlClasses, error && invalidClasses, className)}
          {...props}
        >
          {children}
        </select>
      )}
    </Field>
  )
}

export type CheckboxFieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'id' | 'type'> & {
  label: string
}

export function CheckboxField({ label, className, ...props }: CheckboxFieldProps) {
  const id = useId()
  return (
    <div className="flex min-h-11 items-center gap-2 sm:min-h-9">
      <input id={id} type="checkbox" className={cn('size-4 accent-ink', className)} {...props} />
      <label htmlFor={id} className="text-sm text-ink">
        {label}
      </label>
    </div>
  )
}
