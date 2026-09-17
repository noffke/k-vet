import { useOperatorConfig } from '@/lib/config'

/**
 * Names this instance when it is not the practice's own.
 *
 * Production sets no `environment_label` and nothing is drawn, so the vet's screen is
 * unchanged. Everywhere else — the staging instance one port along on the same Pi, a
 * developer's laptop — this is the only thing telling the two apart, and it has to survive
 * being glanced at: an invoice written in the wrong window leaves no trace, because the record
 * lands in a database nobody looks at and the number it burned was never real.
 *
 * The text is the operator's own (`config.toml`), not translated copy — they know what the
 * instance is for, and a fixed word could not say "nicht für echte Rechnungen".
 */
export function EnvironmentBanner() {
  const label = useOperatorConfig().environment_label?.trim()

  // The backend filters blanks too, but this is the last thing between a stray space in
  // `config.toml` and an empty red stripe across the practice's own screen.
  if (!label) return null

  return (
    <div
      role="status"
      className="bg-danger px-4 py-1 text-center text-sm font-semibold uppercase tracking-[0.12em] text-paper"
    >
      {label}
    </div>
  )
}
