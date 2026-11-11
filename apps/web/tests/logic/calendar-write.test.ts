/**
 * Changing dates on the calendar (lib/logic/calendar.ts): which calendars
 * let a date be changed, and a value moved whole to another first day,
 * keeping its length in days and the time of day of a moment on the
 * reader's clock, over the edges of months and years and the nights the
 * clocks change.
 *
 * The suite runs with the process in three time zones (tools/test-zones.ts);
 * every case names the zone of its reader.
 */
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { datesWritable, movedValue } from '$lib/logic/calendar.ts';
import { wallTimeOf } from '$lib/logic/date-entry.ts';
import {
  addDays,
  calendarDate,
  daysBetween,
  daysOf,
  type CalendarDate,
  type DateValue,
} from '$lib/logic/dates.ts';
import type { PropertyTemplate } from '$lib/logic/wire.ts';

import { at, ZONES } from '../support/zones.ts';

const day = (text: string): CalendarDate => text as CalendarDate;
const on = (text: string): DateValue['from'] => ({ date: day(text) });
const MINUTE = 60_000;

const template = (type: PropertyTemplate['type']): PropertyTemplate => ({
  id: 'due',
  name: 'Due',
  type,
  options: [],
});

describe('which calendars let a date be changed', () => {
  it('one placed by a date property', () => {
    expect(datesWritable(template('date'))).toBe(true);
  });

  it('not one placed by when cards were made or changed, or by nothing', () => {
    expect(datesWritable(template('createdTime'))).toBe(false);
    expect(datesWritable(template('updatedTime'))).toBe(false);
    expect(datesWritable(undefined)).toBe(false);
  });
});

describe('a value moved to another first day', () => {
  const zone = 'Asia/Seoul';

  it('a day becomes the other day', () => {
    expect(movedValue({ from: on('2026-11-12') }, day('2026-11-19'), zone)).toEqual({
      from: on('2026-11-19'),
    });
  });

  it('an empty value becomes that day', () => {
    expect(movedValue({}, day('2026-11-19'), zone)).toEqual({ from: on('2026-11-19') });
  });

  it('a range keeps its length over the end of a month', () => {
    const value = { from: on('2026-11-12'), to: on('2026-11-24') };
    expect(movedValue(value, day('2026-11-26'), zone)).toEqual({
      from: on('2026-11-26'),
      to: on('2026-12-08'),
    });
  });

  it('a range keeps its length over the end of a year and into a leap February', () => {
    const value = { from: on('2027-12-30'), to: on('2028-01-02') };
    expect(movedValue(value, day('2028-02-27'), zone)).toEqual({
      from: on('2028-02-27'),
      to: on('2028-03-01'),
    });
  });

  it('a range moves back as well as on', () => {
    const value = { from: on('2026-12-03'), to: on('2026-12-05') };
    expect(movedValue(value, day('2026-11-29'), zone)).toEqual({
      from: on('2026-11-29'),
      to: on('2026-12-01'),
    });
  });

  it('a range stored backwards comes out in order, its days the same in number', () => {
    const value = { from: on('2026-11-24'), to: on('2026-11-12') };
    expect(movedValue(value, day('2026-11-19'), zone)).toEqual({
      from: on('2026-11-19'),
      to: on('2026-12-01'),
    });
  });

  it('a moment keeps its time of day on the reader’s clock', () => {
    const value = { from: { instant: at(zone, 2026, 11, 10, 8, 0) } };
    expect(movedValue(value, day('2026-11-19'), zone)).toEqual({
      from: { instant: at(zone, 2026, 11, 19, 8, 0) },
    });
  });

  it('a range of moments over midnight keeps both times and its two days', () => {
    const value = {
      from: { instant: at(zone, 2026, 11, 20, 22, 0) },
      to: { instant: at(zone, 2026, 11, 21, 6, 0) },
    };
    expect(movedValue(value, day('2026-11-30'), zone)).toEqual({
      from: { instant: at(zone, 2026, 11, 30, 22, 0) },
      to: { instant: at(zone, 2026, 12, 1, 6, 0) },
    });
  });

  it('is moved by the days of the reader, not those of another zone', () => {
    // 22:00 to 06:00 in Seoul is 05:00 to 13:00 on the 20th in Los Angeles: one day there.
    const la = 'America/Los_Angeles';
    const value = {
      from: { instant: at(zone, 2026, 11, 20, 22, 0) },
      to: { instant: at(zone, 2026, 11, 21, 6, 0) },
    };
    expect(movedValue(value, day('2026-11-22'), la)).toEqual({
      from: { instant: at(la, 2026, 11, 22, 5, 0) },
      to: { instant: at(la, 2026, 11, 22, 13, 0) },
    });
  });

  it('a range of a day and a moment moves both ends', () => {
    const value = { from: on('2026-11-12'), to: { instant: at(zone, 2026, 11, 14, 10, 0) } };
    expect(movedValue(value, day('2026-11-19'), zone)).toEqual({
      from: on('2026-11-19'),
      to: { instant: at(zone, 2026, 11, 21, 10, 0) },
    });
  });
});

describe('a moment moved onto a night the clocks change', () => {
  // New York, 8 March 2026: at 02:00 the clocks go to 03:00.
  const zone = 'America/New_York';

  it('keeps nine in the morning, though the day is 23 hours long', () => {
    const value = { from: { instant: at(zone, 2026, 3, 7, 9, 0) } };
    expect(movedValue(value, day('2026-03-09'), zone)).toEqual({
      from: { instant: at(zone, 2026, 3, 9, 9, 0) },
    });
  });

  it('a start in the skipped hour lands after it, and the end keeps the length in time', () => {
    const value = {
      from: { instant: at(zone, 2026, 3, 7, 2, 30) },
      to: { instant: at(zone, 2026, 3, 7, 3, 10) },
    };
    const moved = movedValue(value, day('2026-03-08'), zone);
    expect(moved.from).toEqual({ instant: at(zone, 2026, 3, 8, 3, 30) });
    expect(moved.to).toEqual({ instant: at(zone, 2026, 3, 8, 3, 30) + 40 * MINUTE });
  });

  it('the second of a time that happens twice lands on that time of a day that has it once', () => {
    // 1 November 2026: 01:30 happens twice; the 2nd has it once.
    const second = Date.UTC(2026, 10, 1, 6, 30);
    const there = movedValue({ from: { instant: second } }, day('2026-11-02'), zone);
    expect(there.from).toEqual({ instant: at(zone, 2026, 11, 2, 1, 30) });
  });
});

describe('a moved value, for any reader', () => {
  it('starts on the day asked for, has as many days, and keeps the times of day', () => {
    fc.assert(
      fc.property(
        fc.constantFrom(...ZONES),
        fc.integer({ min: 2000, max: 2040 }),
        fc.integer({ min: 1, max: 12 }),
        fc.integer({ min: 1, max: 28 }),
        fc.integer({ min: 0, max: 40 }),
        fc.integer({ min: -400, max: 400 }),
        fc.option(fc.integer({ min: 0, max: 24 * 60 - 1 })),
        (zone, year, month, dayOfMonth, length, by, minutes) => {
          const first = calendarDate(year, month, dayOfMonth);
          const last = addDays(first, length);
          const point = (date: CalendarDate): NonNullable<DateValue['from']> =>
            minutes === null
              ? { date }
              : {
                  instant: at(
                    zone,
                    Number(date.slice(0, 4)),
                    Number(date.slice(5, 7)),
                    Number(date.slice(8, 10)),
                    Math.floor(minutes / 60),
                    minutes % 60,
                  ),
                };
          const value =
            length === 0 ? { from: point(first) } : { from: point(first), to: point(last) };
          const before = daysOf(value, zone);
          if (!before) throw new Error('a value with a start has days');
          const target = addDays(before.first, by);
          const moved = movedValue(value, target, zone);
          const after = daysOf(moved, zone);
          expect(after?.first).toBe(target);
          // The length in days is kept, unless a skipped hour pushed an end over midnight.
          if (after)
            expect(daysBetween(after.first, after.last)).toBeGreaterThanOrEqual(
              daysBetween(before.first, before.last),
            );
          if (moved.from?.instant !== undefined && value.from.instant !== undefined) {
            const was = wallTimeOf(value.from.instant, zone);
            const now = wallTimeOf(moved.from.instant, zone);
            // The same time of day, or later on it where the clocks skipped that time.
            expect(now.hour * 60 + now.minute).toBeGreaterThanOrEqual(was.hour * 60 + was.minute);
          }
        },
      ),
      { numRuns: 500 },
    );
  });
});
