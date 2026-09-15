import { execFile } from 'node:child_process'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { expect, type Page } from '@playwright/test'
import { E2E_PASSWORD, E2E_USERNAME } from '../fixtures/credentials'

const run = promisify(execFile)

export interface Raster {
  width: number
  height: number
  /** Raw RGB, three bytes per pixel, row-major from the top left. */
  pixels: Buffer
}

/**
 * Rasterises the first page of a PDF, so what is actually *on* the page can be asserted.
 *
 * pdftotext cannot see an image, which is exactly how a letterhead came to print off the top
 * of the sheet with every text assertion still passing (issues.md 9).
 */
export async function pdfFirstPage(bytes: Buffer, dpi = 40): Promise<Raster> {
  const dir = await mkdtemp(join(tmpdir(), 'kvet-raster-'))
  const file = join(dir, 'page.pdf')
  await writeFile(file, bytes)
  await run('pdftoppm', ['-r', String(dpi), '-f', '1', '-l', '1', file, join(dir, 'page')])
  const ppm = await readFile(join(dir, 'page-1.ppm'))

  // P6: magic, width, height, max value — each followed by whitespace, then raw RGB.
  let at = 0
  const token = () => {
    while (ppm[at] === 0x20 || ppm[at] === 0x0a || ppm[at] === 0x0d || ppm[at] === 0x09) at += 1
    const start = at
    while (at < ppm.length && ppm[at] !== undefined && ppm[at]! > 0x20) at += 1
    return ppm.subarray(start, at).toString('ascii')
  }
  const magic = token()
  if (magic !== 'P6') throw new Error(`expected a P6 bitmap from pdftoppm, got ${magic}`)
  const width = Number(token())
  const height = Number(token())
  token() // max value
  at += 1 // the single whitespace byte before the data

  return { width, height, pixels: ppm.subarray(at) }
}

/**
 * Whether a colour close to `rgb` appears in the top `fraction` of the page. Rendering is
 * lossy, so "close" is what can be asserted — not an exact match.
 */
export function hasColourNearTop(
  raster: Raster,
  rgb: [number, number, number],
  fraction = 0.2,
  tolerance = 40,
): boolean {
  const rows = Math.floor(raster.height * fraction)
  for (let y = 0; y < rows; y += 1) {
    for (let x = 0; x < raster.width; x += 1) {
      const at = (y * raster.width + x) * 3
      const near =
        Math.abs((raster.pixels[at] ?? 0) - rgb[0]) <= tolerance &&
        Math.abs((raster.pixels[at + 1] ?? 0) - rgb[1]) <= tolerance &&
        Math.abs((raster.pixels[at + 2] ?? 0) - rgb[2]) <= tolerance
      if (near) return true
    }
  }
  return false
}

/** Reads a PDF's text with poppler's pdftotext, the same tool the GOT import uses. */
export async function pdfText(bytes: Buffer): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'kvet-pdf-'))
  const file = join(dir, 'invoice.pdf')
  await writeFile(file, bytes)
  const { stdout } = await run('pdftotext', ['-layout', file, '-'])
  return stdout
}

/**
 * A page that scrolls sideways is a layout bug on a phone — and it also shrinks Chrome's
 * page scale, which moves the fixed tab bar out of the viewport. Checked on every
 * navigation so no screen can regress unnoticed (Constitution III).
 */
export async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  expect(overflow, 'the page must fit the viewport width').toBeLessThanOrEqual(0)
}

/**
 * Clicks a navigation entry, whichever chrome the viewport shows: the desktop rail or the
 * phone tab bar (plus its overflow sheet for the secondary sections).
 */
export async function navigate(page: Page, label: string): Promise<void> {
  const link = page.getByRole('link', { name: label }).filter({ visible: true })
  if ((await link.count()) === 0) {
    // Sections that live in the phone overflow sheet.
    await page.getByRole('button', { name: 'Öffnen' }).click()
  }
  await page.getByRole('link', { name: label }).filter({ visible: true }).first().click()
  await expectNoSidewaysScroll(page)
}

/** Signs in through the real login form and waits for the app shell. */
export async function signIn(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByLabel('Benutzername').fill(E2E_USERNAME)
  await page.getByLabel('Passwort').fill(E2E_PASSWORD)
  await page.getByRole('button', { name: 'Anmelden' }).click()
  await page.getByRole('heading', { name: 'Übersicht' }).waitFor()
  await expectNoSidewaysScroll(page)
}
