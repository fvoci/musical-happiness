/**
 * How a day, a month and a time are written, in the language of the page.
 * Nothing here is a word of ours: `Intl` supplies the names of months and
 * weekdays and the order of the parts, so another language needs no table.
 *
 * A calendar date is formatted as that date in UTC: it is a day, not a
 * moment, and must not shift with the reader's zone. A time is formatted in
 * the reader's zone.
 */
import type { CalendarDate, DatePoint } from '$lib/logic/dates.ts';

import type { Month } from './grid.ts';
import { firstOf } from './grid.ts';

const locale = document.documentElement.lang || 'ko';

const utc = (date: CalendarDate): Date => new Date(`${date}T00:00:00Z`);

function formatter(options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(locale, { timeZone: 'UTC', ...options });
}

const MONTH = formatter({ year: 'numeric', month: 'long' });
const DAY = formatter({ month: 'long', day: 'numeric' });
const DAY_LONG = formatter({ month: 'long', day: 'numeric', weekday: 'long' });
const WEEKDAY = formatter({ weekday: 'short' });
const NUMBER = formatter({ day: 'numeric' });

/** "2026년 11월". */
export function monthTitle(month: Month): string {
  return MONTH.format(utc(firstOf(month)));
}

/** "11월 12일". */
export function dayLabel(date: CalendarDate): string {
  return DAY.format(utc(date));
}

/** "11월 12일 목요일". */
export function dayLongLabel(date: CalendarDate): string {
  return DAY_LONG.format(utc(date));
}

/** "11월 8일 – 14일": from one day to another, the shared parts written once. */
export function rangeLabel(first: CalendarDate, last: CalendarDate): string {
  return DAY.formatRange(utc(first), utc(last));
}

/** "목". */
export function weekdayName(date: CalendarDate): string {
  return WEEKDAY.format(utc(date));
}

/** The number of the day in its month, without the word for "day" some languages add. */
export function dayNumber(date: CalendarDate): string {
  return NUMBER.formatToParts(utc(date)).find((part) => part.type === 'day')?.value ?? '';
}

/** "오전 8:00" for a moment, in the reader's zone; nothing for a day without a time. */
export function timeLabel(point: DatePoint | undefined, timeZone: string): string | undefined {
  if (point?.instant === undefined) return undefined;
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', timeZone }).format(
    point.instant,
  );
}
