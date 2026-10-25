import type { TreatmentItem } from '@/api/generated/model'

/**
 * The Wegegeld lines still without kilometres. Without them a line would bill the bare
 * catalogue price, so the server refuses an invoice while there are any — the pages say so
 * before the button rather than after it.
 */
export function linesWithoutKm(items: TreatmentItem[] | undefined): TreatmentItem[] {
  return (items ?? []).filter((item) => item.travel_expenses && item.km == null)
}
