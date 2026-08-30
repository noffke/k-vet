import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import de from '@/i18n/de.json'
import en from '@/i18n/en.json'

/**
 * Nothing in the UI may render an i18n key (issues.md 18).
 *
 * i18next has no `parseMissingKeyHandler` here, so a key missing from *both* files is rendered
 * verbatim — which is how the invoice list came to show `FIELD.TOTAL` (`.eyebrow` uppercases
 * the header). Neither a parity check nor a "no empty values" check would have caught that:
 * `field.total` existed in neither file, so the two stayed perfectly in step. The scan of the
 * call sites is the part that catches it.
 */

type Tree = { [key: string]: string | Tree }

const SRC = resolve(__dirname, '../src')
const BACKEND_SRC = resolve(__dirname, '../../k-vet-backend/src')

/**
 * Keys built from a template literal cannot be resolved statically. Each one is listed with the
 * prefix its values share, so a *new* dynamic key is a deliberate addition to this list rather
 * than a silent hole in the check.
 */
const DYNAMIC_KEY_PREFIXES = [
  'emailType.',
  'field.',
  'invoices.status_',
  'pharmacy.flags.',
  'salutation.',
  'sex.',
]

function leaves(tree: Tree, prefix = ''): Map<string, string> {
  const out = new Map<string, string>()
  for (const [key, value] of Object.entries(tree)) {
    const path = `${prefix}${key}`
    if (typeof value === 'string') out.set(path, value)
    else for (const [k, v] of leaves(value, `${path}.`)) out.set(k, v)
  }
  return out
}

function sourceFiles(dir: string, extensions: string[]): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      // Generated clients carry no user-facing strings.
      return entry === 'generated' || entry === 'node_modules' ? [] : sourceFiles(path, extensions)
    }
    return extensions.some((extension) => path.endsWith(extension)) ? [path] : []
  })
}

const german = leaves(de as Tree)
const english = leaves(en as Tree)

describe('translation files', () => {
  it('carry the same keys in both languages', () => {
    expect([...german.keys()].filter((key) => !english.has(key))).toEqual([])
    expect([...english.keys()].filter((key) => !german.has(key))).toEqual([])
  })

  it('have no empty values', () => {
    for (const [key, value] of [...german, ...english]) {
      expect(value.trim(), `${key} is empty`).not.toBe('')
    }
  })

  it('interpolate the same placeholders in both languages', () => {
    const placeholders = (value: string) =>
      [...value.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1]).sort()
    for (const [key, value] of german) {
      expect(placeholders(english.get(key) ?? ''), `${key} placeholders differ`).toEqual(
        placeholders(value),
      )
    }
  })
})

describe('every key the code asks for exists', () => {
  it('for literal t() calls in the frontend', () => {
    const missing: string[] = []
    for (const file of sourceFiles(SRC, ['.ts', '.tsx'])) {
      const source = readFileSync(file, 'utf8')
      for (const match of source.matchAll(/\bt\(\s*'([^']+)'/g)) {
        const key = match[1] as string
        if (!german.has(key)) missing.push(`${file.replace(SRC, 'src')}: ${key}`)
      }
    }
    expect(missing).toEqual([])
  })

  it('for the message keys the backend returns as field errors', () => {
    // These reach the UI through `t(autoSave.fieldErrors[key])`, so an untranslated one renders
    // as the bare key in a form — the static scan above cannot see them.
    const missing: string[] = []
    for (const file of sourceFiles(BACKEND_SRC, ['.rs'])) {
      const source = readFileSync(file, 'utf8')
      for (const match of source.matchAll(/AppError::field\(\s*"[^"]*"\s*,\s*"([^"]+)"/g)) {
        const key = match[1] as string
        if (!german.has(key)) missing.push(`${file.replace(BACKEND_SRC, 'src')}: ${key}`)
      }
    }
    expect(missing).toEqual([])
  })

  it('for every value a dynamic key can take', () => {
    // The template-literal call sites resolve at runtime; assert each prefix has entries, so a
    // renamed namespace is caught even though the individual key cannot be.
    for (const prefix of DYNAMIC_KEY_PREFIXES) {
      const matches = [...german.keys()].filter((key) => key.startsWith(prefix))
      expect(matches.length, `no keys under ${prefix}`).toBeGreaterThan(0)
    }
  })
})
