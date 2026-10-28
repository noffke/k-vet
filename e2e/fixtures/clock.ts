/**
 * The practice's time zone, which the whole suite runs in: the browser (`timezoneId` in
 * playwright.config.ts) and the backend (`TZ` in global-setup.ts, as the image sets it).
 *
 * The test process itself runs wherever it is started — UTC on GitHub's runners — so a spec
 * works out "today" here rather than with a bare `new Date()`: between 22:00 and midnight UTC the
 * two are a day apart, and a spec that asked its own clock failed only at night.
 */
export const TIME_ZONE = 'Europe/Berlin'

const pad = (value: number) => String(value).padStart(2, '0')

/** The practice's calendar day, `offsetDays` from today. */
function practiceDay(offsetDays: number): { day: string; month: string; year: string } {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: TIME_ZONE,
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
  }).formatToParts(new Date())
  const part = (type: string) => Number(parts.find((entry) => entry.type === type)?.value)
  // Counted on the calendar, not in hours, so a day across a DST change is still one day.
  const date = new Date(Date.UTC(part('year'), part('month') - 1, part('day') + offsetDays))
  return {
    day: pad(date.getUTCDate()),
    month: pad(date.getUTCMonth() + 1),
    year: String(date.getUTCFullYear()),
  }
}

/** "25.10.2026": the practice's today, or `offsetDays` later, as a German page writes it. */
export function germanDate(offsetDays = 0): string {
  const { day, month, year } = practiceDay(offsetDays)
  return `${day}.${month}.${year}`
}

/** "10/25/2026": the same day as an en-US page writes it. */
export function americanDate(offsetDays = 0): string {
  const { day, month, year } = practiceDay(offsetDays)
  return `${month}/${day}/${year}`
}

/** The practice's current year. */
export function practiceYear(): string {
  return practiceDay(0).year
}

/**
 * An instant on the practice's day `offsetDays` from today: 10:00 UTC, which is 11:00 or 12:00
 * in Berlin — the same calendar day either side of a DST change.
 */
export function practiceMidday(offsetDays = 0): string {
  const { day, month, year } = practiceDay(offsetDays)
  return `${year}-${month}-${day}T10:00:00Z`
}
