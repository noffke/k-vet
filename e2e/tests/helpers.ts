import { execFile } from 'node:child_process'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { expect, type Locator, type Page } from '../fixtures/test'
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
 * The font faces a PDF embeds, by PostScript name (poppler's pdffonts) — without the subset tag
 * in front and the encoding Typst appends (`-Identity-H`).
 */
export async function pdfFonts(bytes: Buffer): Promise<string[]> {
  const dir = await mkdtemp(join(tmpdir(), 'kvet-pdf-'))
  const file = join(dir, 'invoice.pdf')
  await writeFile(file, bytes)
  const { stdout } = await run('pdffonts', [file])
  return stdout
    .split('\n')
    .slice(2)
    .map((line) => line.split(/\s+/)[0]?.replace(/^[A-Z]{6}\+/, '').replace(/-Identity-H$/, '') ?? '')
    .filter((name) => name !== '')
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
    // Exactly: a date field on the page has "Kalender öffnen".
    await page.getByRole('button', { name: 'Öffnen', exact: true }).click()
  }
  await page.getByRole('link', { name: label }).filter({ visible: true }).first().click()
  await expectNoSidewaysScroll(page)
}

/**
 * Signs in and opens `path`, else the Übersicht. The login goes through the API — the page's
 * requests share its cookies — so a test that wants another page does not load the app twice
 * and the dashboard in between. The form itself is `signInThroughForm`.
 */
export async function signIn(page: Page, path?: string): Promise<void> {
  const response = await page.request.post('/api/auth/login', {
    data: { username: E2E_USERNAME, password: E2E_PASSWORD },
  })
  expect(response.ok(), 'the test login is accepted').toBe(true)
  if (path !== undefined) {
    await page.goto(path)
    return
  }
  await page.goto('/')
  await page.getByRole('heading', { name: 'Übersicht' }).waitFor()
  await expectNoSidewaysScroll(page)
}

/** Signs in through the real login form and waits for the app shell. */
export async function signInThroughForm(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByLabel('Benutzername').fill(E2E_USERNAME)
  await page.getByLabel('Passwort').fill(E2E_PASSWORD)
  await page.getByRole('button', { name: 'Anmelden' }).click()
  await page.getByRole('heading', { name: 'Übersicht' }).waitFor()
  await expectNoSidewaysScroll(page)
}

/**
 * The positions of a list — an animal's "Positionen", the visit's "pro Termin", or a
 * Preisauskunft's. Each is one row; its values and details open on demand (`PositionRow`).
 */
export function positions(page: Page, list = 'Positionen'): Locator {
  return page.getByRole('list', { name: list, exact: true }).getByRole('listitem')
}

/** Every position within `scope` — an animal's tab holds its own and the visit's. */
export function allPositions(scope: Page | Locator): Locator {
  return scope.locator('li:has(> button[aria-expanded])')
}

/** One position's row, in whichever list, found by (part of) its name. */
export function position(page: Page, name: string | RegExp): Locator {
  return page
    .getByRole('listitem')
    .filter({ has: page.locator('button[aria-expanded]', { hasText: name }) })
}

/** Opens what waits behind a position's name — rename, prices, animal, Umwidmung, order. */
export async function openPosition(page: Page, name: string | RegExp): Promise<Locator> {
  const row = position(page, name).first()
  const toggle = row.locator('button[aria-expanded]').first()
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click()
  await expect(toggle).toHaveAttribute('aria-expanded', 'true')
  return row
}

/**
 * The field of one of a position's values — "Menge", "Faktor" or "Kilometer" — opened if it
 * is not already (the line just picked has its values open).
 */
export async function editValue(
  page: Page,
  name: string | RegExp,
  label: 'Menge' | 'Faktor' | 'Kilometer',
): Promise<Locator> {
  const row = position(page, name).first()
  const field = row.getByLabel(label, { exact: true })
  if ((await field.count()) === 0) {
    await row.getByRole('button', { name: new RegExp(`^${label} ändern:`) }).click()
  }
  await expect(field).toBeVisible()
  return field
}

/**
 * Asserts that `inner` lies within `outer` from left to right — a unit inside its field, say,
 * rather than spilling out of it because a narrow column squeezed the field.
 */
export async function expectInside(inner: Locator, outer: Locator): Promise<void> {
  const [box, frame] = await Promise.all([inner.boundingBox(), outer.boundingBox()])
  expect(box, 'the inner element is laid out').not.toBeNull()
  expect(frame, 'the outer element is laid out').not.toBeNull()
  if (!box || !frame) return
  expect(box.x, 'starts inside').toBeGreaterThanOrEqual(frame.x - 0.5)
  expect(box.x + box.width, 'ends inside').toBeLessThanOrEqual(frame.x + frame.width + 0.5)
}
