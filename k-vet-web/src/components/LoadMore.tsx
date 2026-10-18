import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import type { PagedList } from '@/lib/paging'

/**
 * The end of a paged list: "Mehr laden" while the server has more, nothing once it has not —
 * so a list that stops without the button really is complete.
 */
export function LoadMore({
  list,
}: {
  list: Pick<PagedList<unknown>, 'hasMore' | 'loadMore' | 'loadingMore'>
}) {
  const { t } = useTranslation()
  if (!list.hasMore) return null
  return (
    <div className="mt-3 flex justify-center">
      <Button onClick={list.loadMore} disabled={list.loadingMore}>
        {list.loadingMore ? t('list.loading') : t('list.loadMore')}
      </Button>
    </div>
  )
}
