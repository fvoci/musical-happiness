/**
 * The dates of cards that repeat, as the server gives them (and the
 * in-memory backend with these functions): a step counted from the due date
 * passes the days already gone, a month on from its end is the end of a
 * shorter month, and today is the completer's.
 */
import { describe, expect, it } from 'vitest';

import {
  choiceOf,
  nextDueValue,
  notDoneValue,
  readRecurrence,
  recurrenceOfChoice,
  type Recurrence,
} from '$lib/logic/recurrence.ts';
import type { DoneRule, PropertyTemplate } from '$lib/logic/wire.ts';

/** 2027-01-15 12:00 UTC: the 16th already at UTC+14. */
const NOON = Date.UTC(2027, 0, 15, 12);

const every = (count: number, unit: Recurrence['unit'], anchor: Recurrence['anchor'] = 'due') => ({
  every: count,
  unit,
  anchor,
});

describe('the next due date of a card that repeats', () => {
  it('steps from the due date, past the days gone by', () => {
    const weekly = every(1, 'week');
    expect(nextDueValue(weekly, { from: '2027-01-20' }, NOON, 'UTC')).toEqual({
      from: '2027-01-27',
    });
    // Due on the 1st and done two weeks late: the first Friday after today.
    expect(nextDueValue(weekly, { from: '2027-01-01' }, NOON, 'UTC')).toEqual({
      from: '2027-01-22',
    });
    expect(nextDueValue(every(3, 'day'), { from: '2026-12-01' }, NOON, 'UTC')).toEqual({
      from: '2027-01-18',
    });
  });

  it('keeps to the end of a month, each step counted from the first', () => {
    const monthly = every(1, 'month');
    const end = Date.UTC(2027, 0, 31, 12);
    expect(nextDueValue(monthly, { from: '2027-01-31' }, end, 'UTC')).toEqual({
      from: '2027-02-28',
    });
    expect(nextDueValue(monthly, { from: '2028-01-31' }, Date.UTC(2028, 0, 31), 'UTC')).toEqual({
      from: '2028-02-29',
    });
    // Late by two months: three months on from the 31st of January is the 30th of April.
    expect(nextDueValue(monthly, { from: '2027-01-31' }, Date.UTC(2027, 3, 1), 'UTC')).toEqual({
      from: '2027-04-30',
    });
    expect(nextDueValue(every(1, 'year'), { from: '2028-02-29' }, NOON, 'UTC')).toEqual({
      from: '2029-02-28',
    });
  });

  it('counts from the day it is done, in the completer’s zone', () => {
    const daily = every(1, 'day', 'done');
    expect(nextDueValue(daily, { from: '2026-06-01' }, NOON, 'UTC')).toEqual({
      from: '2027-01-16',
    });
    expect(nextDueValue(daily, { from: '2026-06-01' }, NOON, 'Pacific/Kiritimati')).toEqual({
      from: '2027-01-17',
    });
    expect(nextDueValue(every(1, 'week', 'done'), undefined, NOON, 'UTC')).toEqual({
      from: '2027-01-22',
    });
  });

  it('keeps the length of a range and the time of a time', () => {
    const weekly = every(1, 'week');
    expect(nextDueValue(weekly, { from: '2027-01-20', to: '2027-01-22' }, NOON, 'UTC')).toEqual({
      from: '2027-01-27',
      to: '2027-01-29',
    });
    const nine = Date.UTC(2027, 0, 20, 0, 30, 15);
    const next = nextDueValue(weekly, { from: nine, to: nine + 3_600_000 }, NOON, 'Asia/Seoul');
    const week = 7 * 24 * 3_600_000;
    expect(next).toEqual({ from: nine + week, to: nine + week + 3_600_000 });
  });

  it('has none past the year 9999', () => {
    expect(nextDueValue(every(1, 'year'), { from: '9999-06-01' }, NOON, 'UTC')).toBeUndefined();
  });
});

describe('how a card repeats', () => {
  it('reads only a recurrence of 1 to 365 units', () => {
    expect(readRecurrence({ every: 2, unit: 'week', anchor: 'done' })).toEqual(
      every(2, 'week', 'done'),
    );
    for (const wrong of [
      { every: 0, unit: 'day', anchor: 'due' },
      { every: 366, unit: 'day', anchor: 'due' },
      { every: 1.5, unit: 'day', anchor: 'due' },
      { every: 1, unit: 'hour', anchor: 'due' },
      { every: 1, unit: 'day', anchor: 'start' },
      'weekly',
      null,
    ]) {
      expect(readRecurrence(wrong)).toBeUndefined();
    }
  });

  it('is a choice of the editor', () => {
    expect(choiceOf(undefined)).toBe('none');
    expect(choiceOf(every(1, 'month'))).toBe('monthly');
    expect(choiceOf(every(1, 'month', 'done'))).toBe('custom');
    expect(choiceOf(every(2, 'day'))).toBe('custom');
    expect(recurrenceOfChoice('yearly')).toEqual(every(1, 'year'));
    expect(recurrenceOfChoice('none', every(1, 'day'))).toBeUndefined();
    expect(recurrenceOfChoice('custom', every(3, 'day', 'done'))).toEqual(every(3, 'day', 'done'));
  });

  it('starts the next card not done', () => {
    const status: PropertyTemplate = {
      id: 's',
      name: 'Status',
      type: 'select',
      options: [
        { id: 'done', label: 'Done', color: 'green' },
        { id: 'todo', label: 'To do', color: 'blue' },
      ],
    };
    const rule: DoneRule = { kind: 'option', propertyId: 's', optionIds: ['done'] };
    expect(notDoneValue(status, rule, 'done')).toBe('todo');
    const tags = { ...status, type: 'multiSelect' } as PropertyTemplate;
    expect(notDoneValue(tags, rule, ['todo', 'done'])).toEqual(['todo']);
    expect(notDoneValue(tags, rule, ['done'])).toBeUndefined();
    const box = { ...status, type: 'checkbox', options: [] } as PropertyTemplate;
    expect(notDoneValue(box, { kind: 'checkbox', propertyId: 's' }, 'true')).toBeUndefined();
  });
});
