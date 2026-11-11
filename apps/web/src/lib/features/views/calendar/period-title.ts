/**
 * What the calendar's row calls what it shows: the month, or the days of the
 * week with the year (the month's name short, to fit a narrow pane), in the
 * reader's language.
 */
import { firstOf, type Month } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import { displayMonth } from '$lib/logic/format.ts';

import type { Scale } from './period.svelte.ts';

const rangeFormatters = new Map<string, Intl.DateTimeFormat>();

/** Noon of a calendar day in UTC, for a formatter that reads it in UTC. `Date.UTC` would read the years 0 to 99 as 1900 to 1999. */
function utcNoon(date: CalendarDate): Date {
  const day = new Date(0);
  day.setUTCFullYear(
    Number(date.slice(0, 4)),
    Number(date.slice(5, 7)) - 1,
    Number(date.slice(8, 10)),
  );
  day.setUTCHours(12);
  return day;
}

/** What the calendar's row calls what it shows: "November 2026", or "Nov 8 – 14, 2026". */
export function periodTitle(
  scale: Scale,
  weeks: readonly (readonly CalendarDate[])[],
  month: Month,
  locale: string,
): string {
  const week = weeks[0];
  const first = week?.[0];
  const last = week?.at(-1);
  if (scale === 'month' || !first || !last) return displayMonth(firstOf(month), locale);
  let format = rangeFormatters.get(locale);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, {
      timeZone: 'UTC',
      calendar: 'gregory',
      year: 'numeric',
      // Short, so that the week fits the row of a narrow pane: "Nov 8 – 14, 2026".
      month: 'short',
      day: 'numeric',
    });
    rangeFormatters.set(locale, format);
  }
  return format.formatRange(utcNoon(first), utcNoon(last));
}
