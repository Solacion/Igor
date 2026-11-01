/** Named reporting periods, so the model need not know today's date. */
export const PERIODS = ['today', 'yesterday', 'this_week', 'last_7_days', 'this_month', 'last_month', 'last_30_days', 'this_year'] as const
export type PeriodName = typeof PERIODS[number]

/** An inclusive range of local calendar days, as `YYYY-MM-DD`. */
export interface DateRange { from: string, to: string }

/** Longest range any report may cover. */
export const MAX_RANGE_DAYS = 366

export class PeriodError extends Error {}

const DAY_MS = 86_400_000

function iso(date: Date) {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function parse(day: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day)
  const date = match ? new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3])) : undefined
  if (!date || iso(date) !== day)
    throw new PeriodError(`${JSON.stringify(day)} is not a date in YYYY-MM-DD form; for relative days pass period, such as "today"`)
  return date
}

/** The range for `period`, or for `from`/`to`, relative to `now` in local time. Defaults to today. */
export function resolveRange(args: { period?: PeriodName, from?: string, to?: string }, now = new Date()): DateRange {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const shift = (date: Date, days: number) => new Date(date.getFullYear(), date.getMonth(), date.getDate() + days)
  // Small models often put a period name in from; read it as the period.
  if (!args.to && PERIODS.includes(args.from as PeriodName))
    args = { period: args.from as PeriodName }
  let from: Date
  let to: Date
  if (args.from || args.to) {
    from = parse(args.from ?? args.to!)
    to = parse(args.to ?? iso(today))
  }
  else {
    switch (args.period ?? 'today') {
      case 'today': from = to = today; break
      case 'yesterday': from = to = shift(today, -1); break
      // Weeks start on Monday.
      case 'this_week': from = shift(today, -((today.getDay() + 6) % 7)); to = today; break
      case 'last_7_days': from = shift(today, -6); to = today; break
      case 'this_month': from = new Date(today.getFullYear(), today.getMonth(), 1); to = today; break
      case 'last_month': from = new Date(today.getFullYear(), today.getMonth() - 1, 1); to = new Date(today.getFullYear(), today.getMonth(), 0); break
      case 'last_30_days': from = shift(today, -29); to = today; break
      case 'this_year': from = new Date(today.getFullYear(), 0, 1); to = today; break
    }
  }
  if (to < from)
    throw new PeriodError('the end date is before the start date')
  if (to > today)
    throw new PeriodError(`the end date is after today, ${iso(today)}`)
  if (Math.round((to.getTime() - from.getTime()) / DAY_MS) + 1 > MAX_RANGE_DAYS)
    throw new PeriodError(`a report may cover at most ${MAX_RANGE_DAYS} days`)
  return { from: iso(from), to: iso(to) }
}

/** `YYYYMMDD`: the date form SQL Server reads the same under every language setting. */
export function sqlDate(day: string) {
  return day.replaceAll('-', '')
}
