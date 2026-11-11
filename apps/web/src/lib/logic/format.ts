/**
 * Text for dates, durations, numbers and sizes.
 *
 * Nothing here reads the language, the time zone or the clock of the device:
 * the caller passes them, from the reader's settings. A moment is a number
 * of milliseconds since the epoch.
 */
import { dateOfInstant, type CalendarDate } from './dates.ts';

/** How the hours of a time are written: `h12` is 3:20 PM, `h23` is 15:20. */
export type HourCycle = 'h12' | 'h23';

/** What a function needs in order to turn a value into text for a person. */
export interface DisplayContext {
  /** BCP 47 tag, such as `ko` or `en-US`. */
  locale: string;
  /** The reader's time zone, an IANA name such as `Asia/Seoul`. */
  timeZone: string;
  /** The current moment; defaults to the clock. Decides whether a date shows its year. */
  now?: number;
  /** How hours are written; defaults to the habit of the locale. */
  hourCycle?: HourCycle;
  /** Shown for a select value whose option no longer exists. */
  unknownOption?: string;
  /** The title of a card of the board; undefined for a card that is not there (deleted). */
  cardTitle?: (cardId: string) => string | undefined;
}

const isValid = (instant: number): boolean => Number.isFinite(instant);

/** The reader's current year: dates in it are shown without the year. */
function thisYear(context: DisplayContext): number {
  return Number(dateOfInstant(context.now ?? Date.now(), context.timeZone).slice(0, 4));
}

function yearOption(instant: number, context: DisplayContext): 'numeric' | undefined {
  const year = Number(dateOfInstant(instant, context.timeZone).slice(0, 4));
  return year === thisYear(context) ? undefined : 'numeric';
}

/** The day of a moment for the reader: `July 09`, or `July 09, 2025` in another year. */
export function displayDate(instant: number, context: DisplayContext): string {
  if (!isValid(instant)) return '';
  return new Intl.DateTimeFormat(context.locale, {
    timeZone: context.timeZone,
    year: yearOption(instant, context),
    month: 'long',
    day: '2-digit',
  }).format(instant);
}

/**
 * A calendar date as text: `July 09`, with the year unless it is the
 * reader's current year. A calendar date has no time zone, so the day shown
 * is the same for every reader.
 */
export function displayCalendarDate(date: CalendarDate, context: DisplayContext): string {
  const year = Number(date.slice(0, 4));
  return new Intl.DateTimeFormat(context.locale, {
    timeZone: 'UTC',
    year: year === thisYear(context) ? undefined : 'numeric',
    month: 'long',
    day: '2-digit',
  }).format(Date.UTC(year, Number(date.slice(5, 7)) - 1, Number(date.slice(8, 10))));
}

/** The short all-digits form a person types: `07/09/2026` in the United States. */
export function inputDate(instant: number, context: DisplayContext): string {
  if (!isValid(instant)) return '';
  return new Intl.DateTimeFormat(context.locale, {
    timeZone: context.timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(instant);
}

/** A moment on the reader's clock: `July 09 at 3:20 PM`, with the year in another year. */
export function displayDateTime(instant: number, context: DisplayContext): string {
  if (!isValid(instant)) return '';
  return new Intl.DateTimeFormat(context.locale, {
    timeZone: context.timeZone,
    year: yearOption(instant, context),
    month: 'long',
    day: '2-digit',
    hour: 'numeric',
    minute: 'numeric',
    ...(context.hourCycle ? { hourCycle: context.hourCycle } : {}),
  }).format(instant);
}

type CoarseUnit = 'second' | 'minute' | 'hour' | 'day' | 'month' | 'year';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * A length of time as one rounded number and one unit, the way people say
 * it: 50 minutes is "an hour", 25 hours is "a day". Under 45 seconds the
 * count is 0 seconds.
 *
 * Each unit is rounded first and the rounded number decides, with the limits
 * of the library the original used: up to 44 seconds, 44 minutes, 21 hours,
 * 25 days and 10 months stay in their unit.
 */
function coarse(milliseconds: number): [number, CoarseUnit] {
  const length = Math.abs(milliseconds);
  const seconds = Math.round(length / SECOND);
  const minutes = Math.round(length / MINUTE);
  const hours = Math.round(length / HOUR);
  const days = Math.round(length / DAY);
  // 4800 months are 146097 days: the average month of the Gregorian calendar.
  const exactMonths = ((length / DAY) * 4800) / 146097;
  const months = Math.round(exactMonths);
  const years = Math.round(exactMonths / 12);
  if (seconds < 45) return [0, 'second'];
  if (minutes <= 1) return [1, 'minute'];
  if (minutes < 45) return [minutes, 'minute'];
  if (hours <= 1) return [1, 'hour'];
  if (hours < 22) return [hours, 'hour'];
  if (days <= 1) return [1, 'day'];
  if (days < 26) return [days, 'day'];
  if (months <= 1) return [1, 'month'];
  if (months < 11) return [months, 'month'];
  if (years <= 1) return [1, 'year'];
  return [years, 'year'];
}

/** `3 hours ago`, `in 2 days`, `now`: how far a moment is from the current one. */
export function relativeTime(instant: number, locale: string, now: number = Date.now()): string {
  const difference = instant - now;
  const [count, unit] = coarse(difference);
  return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(
    difference < 0 ? -count : count,
    unit,
  );
}

/** `3 hours`, `2 months`: how long a span of time is. */
export function humanizeDuration(milliseconds: number, locale: string): string {
  const [count, unit] = coarse(milliseconds);
  // Under 45 seconds the exact count is more useful than "0 seconds".
  const value = unit === 'second' ? Math.round(Math.abs(milliseconds) / SECOND) : count;
  return new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'long' }).format(value);
}

/** Rounds to a number of decimal places. */
export function roundTo(value: number, decimalPlaces: number): number {
  const factor = 10 ** decimalPlaces;
  return Math.round(value * factor) / factor;
}

const BINARY_UNITS = ['KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB', 'ZiB', 'YiB'];
const DECIMAL_UNITS = ['kB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];

/** `1.5 MiB`; with `si`, powers of 1000 and `MB`. */
export function humanFileSize(bytes: number, si = false, decimals = 1): string {
  const step = si ? 1000 : 1024;
  if (Math.abs(bytes) < step) return `${bytes} B`;
  const units = si ? DECIMAL_UNITS : BINARY_UNITS;
  const factor = 10 ** decimals;
  let size = bytes;
  let unit = -1;
  do {
    size /= step;
    unit += 1;
  } while (Math.round(Math.abs(size) * factor) / factor >= step && unit < units.length - 1);
  return `${size.toFixed(decimals)} ${units[unit] ?? ''}`;
}
