/**
 * Calendar dates, and the value of a date property.
 *
 * A date without a time of day is a day on the calendar, the same day for
 * every reader whatever their time zone: "due on the 7th" is the 7th in
 * Seoul and in Lisbon. It is not a moment. A moment (when a card was created,
 * or a date that carries a time) falls on different days for different
 * readers, and "its day" always means the day in the reader's time zone.
 *
 * Nothing here knows how a date is stored; wire.ts converts.
 */

declare const calendarDateBrand: unique symbol;

/** A day on the calendar, written `YYYY-MM-DD`. Two of them compare as text. */
export type CalendarDate = string & { readonly [calendarDateBrand]: true };

const DAY = 24 * 60 * 60 * 1000;
const pad = (value: number, width: number): string => String(value).padStart(width, '0');

/** The date with these numbers; `month` is 1 to 12. Numbers out of range roll over, as in `Date`. */
export function calendarDate(year: number, month: number, day: number): CalendarDate {
  return dateOfUtc(Date.UTC(year, month - 1, day));
}

/** Reads `YYYY-MM-DD`; undefined for anything else, and for days that do not exist. */
export function parseCalendarDate(text: string): CalendarDate | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!match) return undefined;
  const date = calendarDate(Number(match[1]), Number(match[2]), Number(match[3]));
  return date === text ? date : undefined;
}

/** The UTC day a moment falls on. */
export function dateOfUtc(instant: number): CalendarDate {
  const date = new Date(instant);
  return `${pad(date.getUTCFullYear(), 4)}-${pad(date.getUTCMonth() + 1, 2)}-${pad(date.getUTCDate(), 2)}` as CalendarDate;
}

/** Midnight UTC at the start of a date: a number to do arithmetic with, not a moment anyone lives. */
function utcOf(date: CalendarDate): number {
  return Date.UTC(
    Number(date.slice(0, 4)),
    Number(date.slice(5, 7)) - 1,
    Number(date.slice(8, 10)),
  );
}

const formatters = new Map<string, Intl.DateTimeFormat>();

/**
 * The day a moment falls on for a reader: in `timeZone` (an IANA name), or
 * in the time zone of this device when none is given.
 */
export function dateOfInstant(instant: number, timeZone?: string): CalendarDate {
  if (timeZone === undefined) return dateOfLocal(new Date(instant));
  let formatter = formatters.get(timeZone);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat('en-u-ca-gregory-nu-latn', {
      timeZone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    });
    formatters.set(timeZone, formatter);
  }
  const parts: Record<string, string> = {};
  for (const part of formatter.formatToParts(instant)) parts[part.type] = part.value;
  return calendarDate(Number(parts.year), Number(parts.month), Number(parts.day));
}

/** The day a `Date` shows on this device: what a date picker means by the day that was clicked. */
export function dateOfLocal(date: Date): CalendarDate {
  return calendarDate(date.getFullYear(), date.getMonth() + 1, date.getDate());
}

/** Midnight on this device at the start of a date, for a calendar that works with `Date`. */
export function localMidnight(date: CalendarDate): Date {
  return new Date(
    Number(date.slice(0, 4)),
    Number(date.slice(5, 7)) - 1,
    Number(date.slice(8, 10)),
  );
}

export function addDays(date: CalendarDate, days: number): CalendarDate {
  return dateOfUtc(utcOf(date) + days * DAY);
}

/** How many days `later` is after `earlier`; negative when it is before. */
export function daysBetween(earlier: CalendarDate, later: CalendarDate): number {
  return Math.round((utcOf(later) - utcOf(earlier)) / DAY);
}

/** One end of a date value: a day, or a moment when the value carries a time of day. */
export type DatePoint =
  { date: CalendarDate; instant?: undefined } | { instant: number; date?: undefined };

/** The value of a date property: a day or a moment, or a range from one to another. */
export interface DateValue {
  from?: DatePoint;
  /** The last day or moment of a range, itself included. */
  to?: DatePoint;
}

/** The day of one end of a value, for the reader. */
export function dateOfPoint(point: DatePoint, timeZone?: string): CalendarDate {
  return point.date ?? dateOfInstant(point.instant, timeZone);
}

/** The first and the last day a value covers; the same day twice when it is not a range. */
export function daysOf(
  value: DateValue,
  timeZone?: string,
): { first: CalendarDate; last: CalendarDate } | undefined {
  if (!value.from) return undefined;
  const first = dateOfPoint(value.from, timeZone);
  const last = value.to ? dateOfPoint(value.to, timeZone) : first;
  // A range stored backwards still covers the same days.
  return first <= last ? { first, last } : { first: last, last: first };
}

/**
 * Orders two ends of values: by the day each falls on for the reader; on the
 * same day one without a time comes before one with a time, and two moments
 * keep their order.
 */
export function comparePoints(first: DatePoint, second: DatePoint, timeZone?: string): number {
  const firstDay = dateOfPoint(first, timeZone);
  const secondDay = dateOfPoint(second, timeZone);
  if (firstDay !== secondDay) return firstDay < secondDay ? -1 : 1;
  if (first.instant === undefined || second.instant === undefined) {
    return Number(first.instant !== undefined) - Number(second.instant !== undefined);
  }
  return Math.sign(first.instant - second.instant);
}
