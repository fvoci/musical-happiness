/**
 * The words on the axis of a Gantt chart. None of them is a word of ours:
 * `Intl` gives the names in the reader's language. A calendar date is
 * formatted as that date in UTC: it is a day, not a moment, and must not
 * move with anyone's time zone.
 */
import type { CalendarDate } from '$lib/logic/dates.ts';
import { displayDayNumber, displayMonth } from '$lib/logic/format.ts';
import type { GanttZoom } from '$lib/logic/wire.ts';

const formatters = new Map<string, Intl.DateTimeFormat>();

function format(date: CalendarDate, locale: string, part: 'year' | 'month'): string {
  const key = `${locale}\u0000${part}`;
  let formatter = formatters.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, {
      timeZone: 'UTC',
      ...(part === 'year' ? { year: 'numeric' } : { month: 'short' }),
    });
    formatters.set(key, formatter);
  }
  // `Date.UTC` would read the years 0 to 99 as 1900 to 1999.
  const day = new Date(0);
  day.setUTCFullYear(
    Number(date.slice(0, 4)),
    Number(date.slice(5, 7)) - 1,
    Number(date.slice(8, 10)),
  );
  return formatter.format(day);
}

/** The upper row: `November 2026`, or `2026` where the lower row is months. */
export function upperLabel(zoom: GanttZoom, date: CalendarDate, locale: string): string {
  return zoom === 'month' ? format(date, locale, 'year') : displayMonth(date, locale);
}

/** The lower row: the number of a day (of the first day of a week), or `Nov`. */
export function lowerLabel(zoom: GanttZoom, date: CalendarDate, locale: string): string {
  return zoom === 'month' ? format(date, locale, 'month') : displayDayNumber(date, locale);
}
