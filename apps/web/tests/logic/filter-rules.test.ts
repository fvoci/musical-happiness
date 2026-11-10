/**
 * The rules of the filter that the original's tests do not reach: text is
 * compared without regard to case, and dates are compared as calendar days
 * in the reader's time zone. Each of these replaced a behaviour of the
 * original that was a defect (Q2, Q5, Q16 in PORTING.md).
 */
import { describe, expect, it } from 'vitest';

import { calendarDate } from '$lib/logic/dates.ts';
import { isClauseMet, type FilterContext } from '$lib/logic/filter.ts';
import {
  createCard,
  createFilterClause,
  formatDateValue,
  formatFilterDate,
  setCardProperty,
  type FilterCondition,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

const templates: PropertyTemplate[] = [
  { id: 'notes', name: 'Notes', type: 'text', options: [] },
  { id: 'due', name: 'Due', type: 'date', options: [] },
  { id: 'created', name: 'Created', type: 'createdTime', options: [] },
  { id: 'updated', name: 'Updated', type: 'updatedTime', options: [] },
  { id: 'tags', name: 'Tags', type: 'multiSelect', options: [] },
];

/** Most cases do not involve a moment: any reader will do. */
const utc: FilterContext = { timeZone: 'UTC' };

function met(
  propertyId: string,
  condition: FilterCondition,
  filterValue: string,
  card: {
    title?: string;
    notes?: PropertyValue;
    due?: string;
    createAt?: number;
    updateAt?: number;
  },
  context: FilterContext = utc,
): boolean {
  const created = createCard();
  created.title = card.title ?? '';
  if (card.notes !== undefined) setCardProperty(created, 'notes', card.notes);
  if (card.due !== undefined) setCardProperty(created, 'due', card.due);
  if (card.createAt !== undefined) created.createAt = card.createAt;
  if (card.updateAt !== undefined) created.updateAt = card.updateAt;
  const clause = createFilterClause({ propertyId, condition, values: [filterValue] });
  return isClauseMet(clause, templates, created, context);
}

describe('text conditions do not look at case', () => {
  it('for the title', () => {
    expect(met('title', 'is', 'hello', { title: 'Hello' })).toBe(true);
    expect(met('title', 'contains', 'ELL', { title: 'Hello' })).toBe(true);
    expect(met('title', 'startsWith', 'HE', { title: 'Hello' })).toBe(true);
    expect(met('title', 'endsWith', 'LO', { title: 'Hello' })).toBe(true);
    expect(met('title', 'is', 'hell', { title: 'Hello' })).toBe(false);
  });

  it('for a property value, the same way', () => {
    expect(met('notes', 'is', 'hello', { notes: 'Hello' })).toBe(true);
    expect(met('notes', 'is', 'HELLO', { notes: 'hello' })).toBe(true);
    expect(met('notes', 'contains', 'Hell', { notes: 'HELLO' })).toBe(true);
    expect(met('notes', 'startsWith', 'he', { notes: 'Hello' })).toBe(true);
    expect(met('notes', 'endsWith', 'LO', { notes: 'Hello' })).toBe(true);
    expect(met('notes', 'notContains', 'ELL', { notes: 'Hello' })).toBe(false);
    expect(met('notes', 'notStartsWith', 'lo', { notes: 'Hello' })).toBe(true);
    expect(met('notes', 'is', 'hell', { notes: 'Hello' })).toBe(false);
  });

  it('the same for every reader, whatever their language', () => {
    // In Turkish the lower case of I is a dotless ı. The comparison does not follow the device.
    expect(met('notes', 'is', 'TITLE', { notes: 'title' })).toBe(true);
    expect(met('notes', 'contains', 'straße', { notes: 'Hauptstraße 1' })).toBe(true);
  });

  it('unifies full-width forms and Hangul written as separate jamo', () => {
    expect(met('notes', 'contains', 'abc', { notes: 'ＡＢＣ 상사' })).toBe(true);
    expect(met('notes', 'contains', '한글', { notes: '한글' })).toBe(true);
  });

  it('matches a list value when one of its items matches', () => {
    expect(met('notes', 'contains', 'BE', { notes: ['Alpha', 'Beta'] })).toBe(true);
    expect(met('notes', 'is', 'beta', { notes: ['Alpha', 'Beta'] })).toBe(true);
    expect(met('notes', 'startsWith', 'ga', { notes: ['Alpha', 'Beta'] })).toBe(false);
    expect(met('notes', 'notContains', 'ph', { notes: ['Alpha', 'Beta'] })).toBe(false);
  });

  it('treats a missing value as empty text', () => {
    expect(met('notes', 'contains', 'x', {})).toBe(false);
    expect(met('notes', 'notContains', 'x', {})).toBe(true);
    expect(met('notes', 'is', '', {})).toBe(true);
  });
});

describe('a date property is compared as a calendar day', () => {
  const fifth = calendarDate(2026, 3, 5);
  const day = (date: number): string => formatFilterDate(calendarDate(2026, 3, date));
  const single = formatDateValue({ from: { date: fifth } });
  const range = formatDateValue({ from: { date: fifth }, to: { date: calendarDate(2026, 3, 7) } });

  it('"is" finds the day', () => {
    expect(met('due', 'is', day(5), { due: single })).toBe(true);
    expect(met('due', 'is', day(4), { due: single })).toBe(false);
    expect(met('due', 'is', day(6), { due: single })).toBe(false);
  });

  it('"is" finds the day however the value was written within that day', () => {
    // Stored one minute past noon UTC: still the 5th.
    const offNoon = JSON.stringify({ from: Date.UTC(2026, 2, 5, 12, 1) });
    expect(met('due', 'is', day(5), { due: offNoon })).toBe(true);
    const lateEvening = JSON.stringify({ from: Date.UTC(2026, 2, 5, 23, 59) });
    expect(met('due', 'is', day(5), { due: lateEvening })).toBe(true);
  });

  it('"is" finds every day of a range, its ends included', () => {
    expect([4, 5, 6, 7, 8].map((date) => met('due', 'is', day(date), { due: range }))).toEqual([
      false,
      true,
      true,
      true,
      false,
    ]);
  });

  it('"is before" and "is after" look at the start and the end of the value', () => {
    expect(met('due', 'isBefore', day(6), { due: single })).toBe(true);
    expect(met('due', 'isBefore', day(5), { due: single })).toBe(false);
    expect(met('due', 'isAfter', day(4), { due: single })).toBe(true);
    expect(met('due', 'isAfter', day(5), { due: single })).toBe(false);
    // The range 5th to 7th starts before the 6th and ends after it.
    expect(met('due', 'isBefore', day(6), { due: range })).toBe(true);
    expect(met('due', 'isAfter', day(6), { due: range })).toBe(true);
    expect(met('due', 'isAfter', day(7), { due: range })).toBe(false);
  });

  it('is the same for every reader: a day has no time zone', () => {
    for (const timeZone of ['UTC', 'Asia/Seoul', 'America/Los_Angeles', 'Pacific/Kiritimati']) {
      expect(met('due', 'is', day(5), { due: single }, { timeZone })).toBe(true);
      expect(met('due', 'is', day(6), { due: single }, { timeZone })).toBe(false);
    }
  });

  it('a card without a date is neither on, before nor after a day', () => {
    expect(met('due', 'is', day(5), {})).toBe(false);
    expect(met('due', 'isBefore', day(5), {})).toBe(false);
    expect(met('due', 'isAfter', day(5), {})).toBe(false);
    expect(met('due', 'isSet', '', {})).toBe(false);
    expect(met('due', 'isNotSet', '', {})).toBe(true);
    expect(met('due', 'isSet', '', { due: single })).toBe(true);
  });

  it('does not read what is not a date value', () => {
    // Text left behind by another property type, and the bare number of very old data (Q10).
    expect(met('due', 'isSet', '', { due: 'next week' })).toBe(false);
    expect(met('due', 'isSet', '', { due: '1625553000000' })).toBe(false);
    expect(met('due', 'is', day(5), { due: String(Date.UTC(2026, 2, 5, 12)) })).toBe(false);
  });
});

describe("the day a card was created is the day on the reader's calendar", () => {
  const seventh = formatFilterDate(calendarDate(2023, 12, 7));
  // 03:00 on the 7th in Seoul is 18:00 on the 6th in UTC.
  const earlyOnThe7thInSeoul = Date.UTC(2023, 11, 6, 18, 0);
  // 05:00 on the 8th in Seoul is 20:00 on the 7th in UTC.
  const earlyOnThe8thInSeoul = Date.UTC(2023, 11, 7, 20, 0);
  const seoul = { timeZone: 'Asia/Seoul' };
  const utc = { timeZone: 'UTC' };

  it('in Seoul, a card made at 03:00 on the 7th is of the 7th', () => {
    expect(met('created', 'is', seventh, { createAt: earlyOnThe7thInSeoul }, seoul)).toBe(true);
    expect(met('created', 'isBefore', seventh, { createAt: earlyOnThe7thInSeoul }, seoul)).toBe(
      false,
    );
    expect(met('created', 'is', seventh, { createAt: earlyOnThe8thInSeoul }, seoul)).toBe(false);
    expect(met('created', 'isAfter', seventh, { createAt: earlyOnThe8thInSeoul }, seoul)).toBe(
      true,
    );
  });

  it('for a reader in UTC the same two cards fall on the 6th and the 7th', () => {
    expect(met('created', 'is', seventh, { createAt: earlyOnThe7thInSeoul }, utc)).toBe(false);
    expect(met('created', 'isBefore', seventh, { createAt: earlyOnThe7thInSeoul }, utc)).toBe(true);
    expect(met('created', 'is', seventh, { createAt: earlyOnThe8thInSeoul }, utc)).toBe(true);
  });

  it('holds at the far ends of the time zones', () => {
    // 00:30 on the 7th at UTC+14, and 23:30 on the 7th at UTC-11.
    const kiritimati = Date.UTC(2023, 11, 6, 10, 30);
    const pagoPago = Date.UTC(2023, 11, 8, 10, 30);
    expect(
      met('created', 'is', seventh, { createAt: kiritimati }, { timeZone: 'Pacific/Kiritimati' }),
    ).toBe(true);
    expect(
      met('created', 'is', seventh, { createAt: pagoPago }, { timeZone: 'Pacific/Pago_Pago' }),
    ).toBe(true);
    expect(met('created', 'is', seventh, { createAt: kiritimati }, utc)).toBe(false);
  });

  it('covers the whole day, from the first to the last millisecond', () => {
    const start = Date.UTC(2023, 11, 6, 15, 0); // 00:00 on the 7th in Seoul
    expect(met('created', 'is', seventh, { createAt: start }, seoul)).toBe(true);
    expect(met('created', 'is', seventh, { createAt: start - 1 }, seoul)).toBe(false);
    expect(met('created', 'is', seventh, { createAt: start + 86_400_000 - 1 }, seoul)).toBe(true);
    expect(met('created', 'is', seventh, { createAt: start + 86_400_000 }, seoul)).toBe(false);
  });

  it("gives the same answer wherever the code runs: only the reader's time zone counts", () => {
    // 23:59:59 on the 7th and 00:00:01 on the 7th, on a clock in Seoul.
    const lateInSeoul = Date.UTC(2023, 11, 7, 14, 59, 59);
    const earlyInSeoul = Date.UTC(2023, 11, 6, 15, 0, 1);
    expect(met('created', 'is', seventh, { createAt: lateInSeoul }, seoul)).toBe(true);
    expect(met('created', 'is', seventh, { createAt: earlyInSeoul }, seoul)).toBe(true);
    expect(met('updated', 'is', seventh, { updateAt: earlyInSeoul }, seoul)).toBe(true);
    // For a reader in UTC the second of those is still the 6th.
    expect(met('created', 'is', seventh, { createAt: earlyInSeoul }, utc)).toBe(false);
  });
});
