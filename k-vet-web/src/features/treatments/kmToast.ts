import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { KmSuggestion } from '@/api/generated/model'
import { useLocaleFormat } from '@/lib/locale'
import { scheduleOf } from '@/lib/schedule'

/**
 * Says where a Wegegeld line's kilometres came from, so the vet checks a number she did not
 * type — or why there are none, so she types them. Nothing to say when nothing was looked up:
 * the returned function answers whether it said something.
 *
 * One wording for the treatment's lines and the Preisauskunft's.
 */
export function useKmToast(): (
  suggestion: KmSuggestion | null | undefined,
  name: string,
) => boolean {
  const { t } = useTranslation()
  const { quantity: formatQuantity, date } = useLocaleFormat()

  return (suggestion, name) => {
    if (!suggestion) return false
    if (suggestion.problem) {
      toast.warning(
        t('treatments.kmNotComputed', {
          name,
          reason: t(`treatments.kmProblem.${suggestion.problem}`),
        }),
      )
      return true
    }
    const km = formatQuantity(suggestion.km)
    if (suggestion.source === 'previous') {
      toast(
        suggestion.from_visit
          ? t('treatments.kmFromPrevious', {
              name,
              km,
              // The visit's own day where the practice is, not the UTC one.
              date: date(scheduleOf(suggestion.from_visit).date),
            })
          : t('treatments.kmFromPreviousUndated', { name, km }),
      )
    } else if (suggestion.approximate) {
      toast.warning(t('treatments.kmFromRoute', { name, km }), {
        description: t('treatments.kmApproximate'),
      })
    } else {
      toast(t('treatments.kmFromRoute', { name, km }))
    }
    return true
  }
}
