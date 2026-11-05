import { useQueryClient } from '@tanstack/react-query'
import { Barcode, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/fetcher'
import {
  addDrugCode,
  deleteDrugCode,
  getGetDrugQueryKey,
  getListDrugsQueryKey,
  readScan,
} from '@/api/generated/endpoints'
import type { Drug, DrugCode } from '@/api/generated/model'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ScanField } from '@/components/ScanField'
import { Button } from '@/components/ui/button'

/**
 * The codes on the drug's packs — an EAN, a PZN — by which a barcode scanner finds it at the
 * Wareneingang. A code is scanned (or typed) into the field below the list. A whole Data Matrix
 * may be scanned too: only the product code is kept, since its batch and expiry change with
 * every delivery and belong to the lot.
 */
export function DrugCodes({ drug }: { drug: Drug }) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [problem, setProblem] = useState<string | null>(null)
  const [removing, setRemoving] = useState<DrugCode | null>(null)

  const store = async (updated?: Drug) => {
    if (updated) client.setQueryData(getGetDrugQueryKey(drug.id), updated)
    else await client.invalidateQueries({ queryKey: getGetDrugQueryKey(drug.id) })
    await client.invalidateQueries({ queryKey: getListDrugsQueryKey() })
  }

  const add = async (raw: string) => {
    setProblem(null)
    try {
      // Read first: a code another drug already has is named, rather than refused blind.
      const scanned = await readScan({ raw })
      if (!scanned.gtin) {
        setProblem(t('code.invalid'))
        return
      }
      if (scanned.drug && scanned.drug.id !== drug.id) {
        setProblem(t('pharmacy.codeOfOther', { name: scanned.drug.name ?? '' }))
        return
      }
      await store(await addDrugCode(drug.id, { code: raw }))
    } catch (error) {
      setProblem(
        error instanceof ApiError && error.status === 422 ? t('code.invalid') : t('error.generic'),
      )
    }
  }

  return (
    <section className="mt-6">
      <h2 className="flex items-center gap-2 text-base">
        <Barcode className="size-4" aria-hidden />
        {t('pharmacy.codes')}
      </h2>
      <p className="mt-1 text-sm text-ink-soft">{t('pharmacy.codesHint')}</p>
      {drug.codes.length > 0 ? (
        <ul className="mt-2 flex flex-col gap-2">
          {drug.codes.map((code) => (
            <li
              key={code.id}
              className="flex items-center justify-between gap-2 rounded-card border border-line bg-surface px-3 py-2"
            >
              <span className="numeric text-ink">{code.display}</span>
              <Button
                size="icon"
                variant="ghost"
                aria-label={t('pharmacy.removeCode', { code: code.display })}
                onClick={() => setRemoving(code)}
              >
                <Trash2 className="size-4 text-danger" />
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
      <ScanField
        label={t('pharmacy.addCode')}
        placeholder={t('pharmacy.addCodePlaceholder')}
        wrapperClassName="mt-3"
        error={problem ?? undefined}
        onScan={(raw) => void add(raw)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => {
          if (!open) setRemoving(null)
        }}
        title={t('pharmacy.removeCodeTitle')}
        confirmLabel={t('action.delete')}
        onConfirm={() => {
          const code = removing
          if (code) void deleteDrugCode(code.id).then(() => store())
        }}
      >
        <p>{t('pharmacy.removeCodeConfirm', { code: removing?.display ?? '' })}</p>
      </ConfirmDialog>
    </section>
  )
}
