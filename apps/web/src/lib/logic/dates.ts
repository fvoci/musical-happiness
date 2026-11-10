/**
 * Calendar days and the timestamps that stand for them.
 *
 * A date property without a time of day means a calendar day, the same day
 * for every reader whatever their time zone. It is stored as the timestamp
 * of 12:00 UTC of that day. A reader's calendar works in local time, so the
 * two functions here translate between "12:00 UTC of the day" and "a local
 * time on that day".
 */
import type { DateValue } from './wire.ts';

const offsetAt = (timestamp: number): number => new Date(timestamp).getTimezoneOffset() * 60 * 1000;

/**
 * The timestamp to store for the calendar day that `day` falls on in local
 * time: 12:00 UTC of that day.
 *
 * QUIRK(Q11): only the hour is set to 12; minutes and seconds of `day` are
 * carried into the stored value. A date picker hands over whole hours, so in
 * practice the result is exactly 12:00, which the "is" filter relies on
 * (it compares to the millisecond). Kept as in the original.
 */
export function timestampOfDay(day: Date): number {
  const noon = new Date(day);
  noon.setHours(12);
  return noon.getTime() - offsetAt(noon.getTime());
}

/** The local time that shows the stored day (or, with a time of day, the instant itself). */
export function localDateOf(timestamp: number, includeTime = false): Date {
  return new Date(timestamp + (includeTime ? 0 : offsetAt(timestamp)));
}

function startOfLocalDay(
  timestamp: number | undefined,
  includeTime: boolean | undefined,
): Date | undefined {
  if (!timestamp) return undefined;
  const date = localDateOf(timestamp, includeTime);
  date.setHours(0, 0, 0, 0);
  return date;
}

/** Local midnight of the first day of a date value, for a calendar. */
export function dateFromOf(value: DateValue): Date | undefined {
  return startOfLocalDay(value.from, value.includeTime);
}

/** Local midnight of the last day of a date value, if it is a range. */
export function dateToOf(value: DateValue): Date | undefined {
  return startOfLocalDay(value.to, value.includeTime);
}
