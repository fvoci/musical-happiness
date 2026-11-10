/**
 * The arithmetic of a month drawn as weeks: which days a month's grid shows,
 * and where the bar of a card goes in a week.
 *
 * Days are calendar dates (`$lib/logic/dates.ts`); nothing here reads a
 * clock or a time zone. Which day a card is on was settled before this, by
 * the reader's time zone, and the first day of a week is an argument.
 *
 * It lives in the spike because the spike may not touch the product; the
 * product will want this in `src/lib/logic`, with tests.
 */
import { addDays, calendarDate, daysBetween, type CalendarDate } from '$lib/logic/dates.ts';
import type { Weekday } from '$lib/logic/regional.ts';

export interface Month {
  year: number;
  /** 1 to 12. */
  month: number;
}

export function monthOf(date: CalendarDate): Month {
  return { year: Number(date.slice(0, 4)), month: Number(date.slice(5, 7)) };
}

export function firstOf(month: Month): CalendarDate {
  return calendarDate(month.year, month.month, 1);
}

export function lastOf(month: Month): CalendarDate {
  // Day 0 of the next month rolls back to the last day of this one.
  return calendarDate(month.year, month.month + 1, 0);
}

export function shiftMonth(month: Month, by: number): Month {
  return monthOf(calendarDate(month.year, month.month + by, 1));
}

export function inMonth(date: CalendarDate, month: Month): boolean {
  return date >= firstOf(month) && date <= lastOf(month);
}

/** The day of the week as ISO 8601 numbers it: 1 is Monday, 7 is Sunday. */
export function weekdayOf(date: CalendarDate): Weekday {
  // 1970-01-01 was a Thursday.
  const sinceThursday = daysBetween(calendarDate(1970, 1, 1), date);
  return (((((sinceThursday + 3) % 7) + 7) % 7) + 1) as Weekday;
}

/** The seven days of the week `date` is in, for a reader whose week starts on `weekStart`. */
export function weekOf(date: CalendarDate, weekStart: Weekday): CalendarDate[] {
  const back = (weekdayOf(date) - weekStart + 7) % 7;
  const first = addDays(date, -back);
  return Array.from({ length: 7 }, (_, at) => addDays(first, at));
}

/** The weeks a month's grid shows: every week that has a day of the month in it. */
export function weeksOfMonth(month: Month, weekStart: Weekday): CalendarDate[][] {
  const weeks: CalendarDate[][] = [];
  const last = lastOf(month);
  for (let day = firstOf(month); ; day = addDays(day, 7)) {
    const week = weekOf(day, weekStart);
    if ((week[0] ?? day) > last) break;
    weeks.push(week);
  }
  return weeks;
}

/** The days a card is on: the same day twice when it is not a range. */
export interface Span {
  id: string;
  first: CalendarDate;
  last: CalendarDate;
}

/** The part of a card's span that falls in one week, as a bar. */
export interface Segment {
  id: string;
  /** The row of bars it is in, from 0 at the top. */
  lane: number;
  /** The first column it covers, 0 to 6, and how many. */
  column: number;
  length: number;
  /** The card began before this week, or goes on after it. */
  fromBefore: boolean;
  toAfter: boolean;
}

/**
 * The bars of one week. A card that began earlier comes first, then the
 * longer one, then the order given; each takes the highest row that is free
 * on all its days. Rows are worked out a week at a time: a card that spans
 * two weeks may sit in different rows of the two.
 */
export function layWeek(week: readonly CalendarDate[], spans: readonly Span[]): Segment[] {
  const start = week[0];
  const end = week[6];
  if (!start || !end) return [];
  const here = spans
    .filter((span) => span.first <= end && span.last >= start)
    .map((span, order) => ({ span, order }))
    .sort(
      (a, b) =>
        (a.span.first < b.span.first ? -1 : a.span.first > b.span.first ? 1 : 0) ||
        daysBetween(b.span.first, b.span.last) - daysBetween(a.span.first, a.span.last) ||
        a.order - b.order,
    );
  /** For each row, the columns taken. */
  const taken: boolean[][] = [];
  return here.map(({ span }) => {
    const column = Math.max(0, daysBetween(start, span.first));
    const until = Math.min(6, daysBetween(start, span.last));
    let lane = 0;
    const free = (row: boolean[] | undefined): boolean =>
      !row || row.slice(column, until + 1).every((cell) => !cell);
    while (!free(taken[lane])) lane += 1;
    const row = (taken[lane] ??= Array.from({ length: 7 }, () => false));
    for (let at = column; at <= until; at += 1) row[at] = true;
    return {
      id: span.id,
      lane,
      column,
      length: until - column + 1,
      fromBefore: span.first < start,
      toAfter: span.last > end,
    };
  });
}

export interface Folded {
  shown: Segment[];
  /** For each column, how many cards of that day have no bar. */
  more: number[];
}

/**
 * What of a week's bars is drawn when a cell has room for `lanes` rows. When
 * a day has more cards than rows, its last row says "+N" instead of a bar;
 * a bar in the last row stays only if none of its days needs that row.
 */
export function foldWeek(segments: readonly Segment[], lanes: number): Folded {
  const covers = (segment: Segment, column: number): boolean =>
    column >= segment.column && column < segment.column + segment.length;
  const columns = Array.from({ length: 7 }, (_, column) => column);
  /** How many rows each day would need. */
  const depth = columns.map((column) =>
    Math.max(0, ...segments.filter((s) => covers(s, column)).map((s) => s.lane + 1)),
  );
  const fits = (segment: Segment): boolean =>
    segment.lane < lanes - 1 ||
    (segment.lane === lanes - 1 &&
      columns.every((column) => !covers(segment, column) || (depth[column] ?? 0) <= lanes));
  const shown = segments.filter(fits);
  const more = columns.map(
    (column) => segments.filter((s) => covers(s, column) && !fits(s)).length,
  );
  return { shown, more };
}

/** The ids of the cards on a day, in the order of their bars. */
export function idsOn(date: CalendarDate, spans: readonly Span[]): string[] {
  return spans.filter((span) => span.first <= date && span.last >= date).map((span) => span.id);
}
