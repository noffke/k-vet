/**
 * "Später versenden": a mail scheduled for a day and an hour, in the browser's timezone — the
 * practice's. The wire carries the instant (ISO, UTC); the form edits a date and an hour.
 */

/** A day (`yyyy-MM-dd`) and an hour of it (0–23). */
export interface Schedule {
  date: string
  hour: number
}

const pad = (value: number) => String(value).padStart(2, '0')

/** The local day of `moment` as `yyyy-MM-dd`. */
function localDate(moment: Date): string {
  return `${moment.getFullYear()}-${pad(moment.getMonth() + 1)}-${pad(moment.getDate())}`
}

/** Tomorrow at eight: the next morning's post. */
export function defaultSchedule(now: Date = new Date()): Schedule {
  const tomorrow = new Date(now)
  tomorrow.setDate(tomorrow.getDate() + 1)
  return { date: localDate(tomorrow), hour: 8 }
}

/** The instant of `schedule`, as the API takes it. */
export function scheduleAt(schedule: Schedule): string {
  const moment = new Date(`${schedule.date}T00:00:00`)
  moment.setHours(schedule.hour, 0, 0, 0)
  return moment.toISOString()
}

/** The day and hour of an instant — to reopen a mail that is already scheduled. */
export function scheduleOf(iso: string): Schedule {
  const moment = new Date(iso)
  return { date: localDate(moment), hour: moment.getHours() }
}

/** Whether an instant is still ahead. */
export function isAhead(iso: string | null | undefined, now: Date = new Date()): boolean {
  return Boolean(iso) && new Date(iso as string).getTime() > now.getTime()
}
