/**
 * The arithmetic of the Gantt view (lib/logic/gantt.ts): the axis for each
 * zoom and first day of the week, across the ends of months and years, its
 * marks, the rows, where a bar lies, which edges are drawn and marked, and
 * the way an arrow takes.
 *
 * The suite runs with the process in three time zones (tools/test-zones.ts);
 * nothing here asks the machine for its own, so every result is the same.
 */
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { spansOf, weekdayOf } from '$lib/logic/calendar.ts';
import { addDays, calendarDate, daysBetween, type CalendarDate } from '$lib/logic/dates.ts';
import type { Days } from '$lib/logic/dependencies.ts';
import {
  ARROW_GAP,
  arrowHead,
  arrowPath,
  arrowPoints,
  arrowsOf,
  axisOf,
  barBox,
  barsOf,
  DAY_WIDTH,
  dayAt,
  edgeKey,
  GANTT_ZOOMS,
  MAX_AXIS_DAYS,
  MIN_BAR_WIDTH,
  relationsOf,
  rowsOf,
  scaleOf,
  xOfDay,
  zoomOf,
  type Axis,
  type Bar,
} from '$lib/logic/gantt.ts';
import type { Weekday } from '$lib/logic/regional.ts';
import {
  createCard,
  setCardProperty,
  type Card,
  type GanttZoom,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

const day = (text: string): CalendarDate => text as CalendarDate;
const days = (first: string, last = first): Days => ({ first: day(first), last: day(last) });
const SUNDAY = 7;
const MONDAY = 1;
const SATURDAY = 6;
const TODAY = day('2026-11-10'); // a Tuesday

function axis(zoom: GanttZoom, spans: Days[] = [], today = TODAY, weekStart: Weekday = SUNDAY) {
  return axisOf({ zoom, weekStart, today, spans });
}

const sum = (widths: { width: number }[]): number =>
  widths.reduce((total, each) => total + each.width, 0);

describe('the zoom', () => {
  it('is the word stored, and `week` for none or one that is not known', () => {
    expect(zoomOf('day')).toBe('day');
    expect(zoomOf('month')).toBe('month');
    expect(zoomOf(undefined)).toBe('week');
    expect(zoomOf('quarter')).toBe('week');
    expect(zoomOf(3)).toBe('week');
  });

  it('makes a day narrower at each step', () => {
    const widths = GANTT_ZOOMS.map((zoom) => DAY_WIDTH[zoom]);
    expect(widths).toEqual([...widths].sort((a, b) => b - a));
  });
});

describe('the axis', () => {
  it('has today on it when there are no cards, with whole weeks around it', () => {
    const made = axis('day');
    // A week of margin either side of Tuesday the 10th, out to whole weeks from Sunday.
    expect(made.first).toBe('2026-11-01');
    expect(made.last).toBe('2026-11-21');
    expect(made.days).toBe(21);
    expect(made.width).toBe(21 * DAY_WIDTH.day);
  });

  it('begins on the first day of the reader`s week', () => {
    expect(axis('day', [], TODAY, MONDAY).first).toBe('2026-11-02');
    expect(axis('day', [], TODAY, SATURDAY).first).toBe('2026-10-31');
    expect(axis('week', [], TODAY, MONDAY).first).toBe('2026-10-26');
    for (const weekStart of [SUNDAY, MONDAY, SATURDAY] as const) {
      const made = axis('week', [], TODAY, weekStart);
      expect(weekdayOf(made.first)).toBe(weekStart);
      expect(made.days % 7).toBe(0);
    }
  });

  it('reaches from the first card to the last, across the end of a year', () => {
    const made = axis('week', [days('2026-12-28', '2027-01-05'), days('2026-10-01')]);
    // Two weeks of margin: 17 September back to Sunday the 13th, 19 January on to Saturday the 23rd.
    expect(made.first).toBe('2026-09-13');
    expect(made.last).toBe('2027-01-23');
  });

  it('is whole months at the zoom `month`, February of a leap year too', () => {
    const made = axis('month', [days('2028-02-10', '2028-02-20')], day('2028-02-15'));
    // 31 days of margin: 10 January and 22 March.
    expect(made.first).toBe('2028-01-01');
    expect(made.last).toBe('2028-03-31');
    expect(made.days).toBe(31 + 29 + 31);
  });

  it('is as long as the page has room for', () => {
    const made = axisOf({ zoom: 'day', weekStart: SUNDAY, today: TODAY, spans: [], minDays: 60 });
    expect(made.first).toBe('2026-11-01');
    expect(made.days).toBeGreaterThanOrEqual(60);
    expect(made.days % 7).toBe(0);
  });

  it('is a window around today when the cards are too far apart to draw', () => {
    for (const zoom of GANTT_ZOOMS) {
      const made = axis(zoom, [days('0001-01-01'), days('9999-12-31')]);
      expect(made.first <= TODAY && TODAY <= made.last).toBe(true);
      // No more than the limit and the rounding out to whole weeks or months.
      expect(made.days).toBeLessThanOrEqual(MAX_AXIS_DAYS[zoom] + 62);
      expect(made.days).toBeGreaterThanOrEqual(MAX_AXIS_DAYS[zoom]);
      // Today is a third of the way in.
      const before = daysBetween(made.first, TODAY) / made.days;
      expect(before).toBeGreaterThan(0.3);
      expect(before).toBeLessThan(0.37);
    }
  });

  it('gives the whole window to the side that has cards', () => {
    const past = axis('day', [days('0001-01-01')]);
    expect(past.last).toBe('2026-11-21');
    expect(past.days).toBeGreaterThanOrEqual(MAX_AXIS_DAYS.day);
    const future = axis('day', [days('9999-12-31')]);
    expect(future.first).toBe('2026-11-01');
    expect(future.days).toBeGreaterThanOrEqual(MAX_AXIS_DAYS.day);
  });

  it('does not leave the years that can be written', () => {
    const early = axis('week', [], day('0001-01-03'));
    expect(early.first).toBe('0001-01-01');
    const late = axis('week', [], day('9999-12-30'));
    expect(late.last).toBe('9999-12-31');
    expect(axis('month', [], day('9999-12-30')).last).toBe('9999-12-31');
    // The marks of an axis cut short at an end still add up to it.
    for (const made of [early, late]) {
      const scale = scaleOf(made, SUNDAY);
      expect(sum(scale.lower)).toBeCloseTo(made.width);
      expect(sum(scale.upper)).toBeCloseTo(made.width);
    }
  });

  it('knows where a day begins and which day lies at a place', () => {
    const made = axis('day');
    expect(xOfDay(made, day('2026-11-01'))).toBe(0);
    expect(xOfDay(made, TODAY)).toBe(9 * DAY_WIDTH.day);
    expect(xOfDay(made, day('2026-10-31'))).toBe(-DAY_WIDTH.day);
    expect(dayAt(made, 9 * DAY_WIDTH.day)).toBe(TODAY);
    expect(dayAt(made, 10 * DAY_WIDTH.day - 0.01)).toBe(TODAY);
    expect(dayAt(made, -5)).toBe(made.first);
    expect(dayAt(made, made.width + 5)).toBe(made.last);
  });
});

describe('the marks of the axis', () => {
  it('are days under months at the zoom `day`, with a line at each week', () => {
    // 25 October to 12 December: the margin of a card from 3 November to 2 December.
    const made = axis('day', [days('2026-11-03', '2026-12-02')]);
    const scale = scaleOf(made, SUNDAY);
    expect(scale.lower).toHaveLength(made.days);
    expect(scale.lower[1]).toEqual({ date: '2026-10-26', x: DAY_WIDTH.day, width: DAY_WIDTH.day });
    expect(scale.upper.map((tick) => tick.date)).toEqual([
      '2026-10-25',
      '2026-11-01',
      '2026-12-01',
    ]);
    expect(scale.upper.map((tick) => tick.width / DAY_WIDTH.day)).toEqual([7, 30, 12]);
    expect(scale.lines).toHaveLength(made.days / 7 - 1);
    expect(scale.lines[0]).toBe(7 * DAY_WIDTH.day);
  });

  it('are weeks under months at the zoom `week`', () => {
    const made = axis('week', [], TODAY, MONDAY);
    const scale = scaleOf(made, MONDAY);
    expect(scale.lower.map((tick) => tick.date)).toEqual([
      '2026-10-26',
      '2026-11-02',
      '2026-11-09',
      '2026-11-16',
      '2026-11-23',
    ]);
    expect(scale.lower.every((tick) => tick.width === 7 * DAY_WIDTH.week)).toBe(true);
    // A month begins in the middle of a week: its mark begins there too.
    expect(scale.upper.map((tick) => [tick.date, tick.x])).toEqual([
      ['2026-10-26', 0],
      ['2026-11-01', 6 * DAY_WIDTH.week],
    ]);
    expect(scale.lines).toEqual(scale.lower.slice(1).map((tick) => tick.x));
  });

  it('are months under years at the zoom `month`, each as wide as its days', () => {
    const made = axis('month', [days('2027-12-20', '2028-03-05')], day('2028-01-15'));
    const scale = scaleOf(made, SUNDAY);
    expect(
      scale.lower.map((tick) => [tick.date, Math.round(tick.width / DAY_WIDTH.month)]),
    ).toEqual([
      ['2027-11-01', 30],
      ['2027-12-01', 31],
      ['2028-01-01', 31],
      ['2028-02-01', 29],
      ['2028-03-01', 31],
      ['2028-04-01', 30],
    ]);
    expect(scale.upper.map((tick) => tick.date)).toEqual(['2027-11-01', '2028-01-01']);
    expect(scale.upper[0]?.width).toBeCloseTo(61 * DAY_WIDTH.month);
  });

  it('add up to the axis and keep today on it, whatever the cards', () => {
    const date = fc
      .integer({ min: -4000, max: 4000 })
      .map((offset) => addDays(calendarDate(2026, 1, 1), offset));
    const span = fc.tuple(date, fc.nat(400)).map(([first, length]) => ({
      first,
      last: addDays(first, length),
    }));
    fc.assert(
      fc.property(
        fc.constantFrom(...GANTT_ZOOMS),
        fc.constantFrom<Weekday>(SUNDAY, MONDAY, SATURDAY),
        date,
        fc.array(span, { maxLength: 6 }),
        (zoom, weekStart, today, spans) => {
          const made = axisOf({ zoom, weekStart, today, spans });
          const scale = scaleOf(made, weekStart);
          expect(made.first <= today && today <= made.last).toBe(true);
          expect(made.days).toBe(daysBetween(made.first, made.last) + 1);
          expect(sum(scale.lower)).toBeCloseTo(made.width, 6);
          expect(sum(scale.upper)).toBeCloseTo(made.width, 6);
          if (zoom === 'month') expect(made.first.slice(8)).toBe('01');
          else expect(weekdayOf(made.first)).toBe(weekStart);
          for (const each of spans) {
            const box = barBox(made, each);
            expect(box.x).toBeGreaterThanOrEqual(0);
            expect(box.x + box.width).toBeLessThanOrEqual(made.width + 1e-9);
          }
        },
      ),
    );
  });
});

describe('rows', () => {
  it('are the cards in the order given where the view is not grouped', () => {
    expect(rowsOf([{ cardIds: ['a', 'b'] }])).toEqual([
      { kind: 'card', id: 'a', row: 0 },
      { kind: 'card', id: 'b', row: 1 },
    ]);
  });

  it('have a heading for each group, an empty one too', () => {
    expect(
      rowsOf([
        { groupId: 'todo', cardIds: ['a'] },
        { groupId: '', cardIds: [] },
        { groupId: 'done', cardIds: ['b', 'c'] },
      ]),
    ).toEqual([
      { kind: 'group', id: 'todo', row: 0, count: 1 },
      { kind: 'card', id: 'a', row: 1 },
      { kind: 'group', id: '', row: 2, count: 0 },
      { kind: 'group', id: 'done', row: 3, count: 2 },
      { kind: 'card', id: 'b', row: 4 },
      { kind: 'card', id: 'c', row: 5 },
    ]);
  });
});

describe('a bar', () => {
  const made: Axis = axis('day'); // 1 to 21 November

  it('of one day is one day wide, on its day', () => {
    expect(barBox(made, days('2026-11-10'))).toEqual({
      x: 9 * DAY_WIDTH.day,
      width: DAY_WIDTH.day,
      fromBefore: false,
      toAfter: false,
    });
  });

  it('of several days reaches from the start of the first to the end of the last', () => {
    const box = barBox(made, days('2026-11-03', '2026-11-05'));
    expect(box.x).toBe(2 * DAY_WIDTH.day);
    expect(box.width).toBe(3 * DAY_WIDTH.day);
  });

  it('is cut at the edge of the axis and says which end is cut', () => {
    expect(barBox(made, days('2026-10-20', '2026-11-02'))).toEqual({
      x: 0,
      width: 2 * DAY_WIDTH.day,
      fromBefore: true,
      toAfter: false,
    });
    const after = barBox(made, days('2026-11-20', '2027-01-01'));
    expect(after).toMatchObject({ x: 19 * DAY_WIDTH.day, fromBefore: false, toAfter: true });
    expect(after.x + after.width).toBe(made.width);
    expect(barBox(made, days('2026-01-01', '2027-01-01'))).toEqual({
      x: 0,
      width: made.width,
      fromBefore: true,
      toAfter: true,
    });
  });

  it('wholly off the axis keeps a stub at the edge it is beyond', () => {
    expect(barBox(made, days('2025-03-01', '2025-03-09'))).toEqual({
      x: 0,
      width: MIN_BAR_WIDTH,
      fromBefore: true,
      toAfter: false,
    });
    expect(barBox(made, days('2027-03-01'))).toEqual({
      x: made.width - MIN_BAR_WIDTH,
      width: MIN_BAR_WIDTH,
      fromBefore: false,
      toAfter: true,
    });
  });

  it('is never too narrow to see: a day at the zoom `month` is widened', () => {
    const months = axis('month');
    const box = barBox(months, days('2026-11-10'));
    expect(box.width).toBe(MIN_BAR_WIDTH);
    expect(box.x).toBeCloseTo(xOfDay(months, TODAY));
    // The last day of the axis: the widened bar ends with the axis.
    const last = barBox(months, { first: months.last, last: months.last });
    expect(last.x + last.width).toBeCloseTo(months.width);
  });

  it('lies on the day a moment falls on for the reader', () => {
    const due: PropertyTemplate = { id: 'due', name: 'Due', type: 'date', options: [] };
    const card = createCard();
    // 01:00 on the 10th in Seoul, 16:00 on the 9th in UTC, 06:00 on the 10th in Kiritimati.
    setCardProperty(card, 'due', { from: Date.parse('2026-11-09T16:00:00Z') });
    const at = (timeZone: string): number => {
      const [span] = spansOf([card], due, timeZone).spans;
      if (!span) throw new Error('the card has no days');
      return barBox(made, span).x / DAY_WIDTH.day;
    };
    expect(at('Asia/Seoul')).toBe(9);
    expect(at('UTC')).toBe(8);
    expect(at('Pacific/Kiritimati')).toBe(9);
  });

  it('is in the row of its card, and a card without days has none', () => {
    const rows = rowsOf([{ groupId: 'g', cardIds: ['a', 'undated', 'b'] }]);
    const spans: Record<string, Days> = { a: days('2026-11-03'), b: days('2026-11-04') };
    const bars = barsOf(made, rows, (id) => spans[id]);
    expect([...bars.keys()]).toEqual(['a', 'b']);
    expect(bars.get('a')).toMatchObject({ id: 'a', row: 1, x: 2 * DAY_WIDTH.day });
    expect(bars.get('b')?.row).toBe(3);
  });
});

const AFTER = 'after';

function card(id: string, after?: string[], other?: PropertyValue): Card {
  const made = createCard();
  made.id = id;
  if (after) setCardProperty(made, AFTER, after);
  if (other !== undefined) setCardProperty(made, 'other', other);
  return made;
}

describe('the relations a chart draws', () => {
  const spans: Record<string, Days> = {
    a: days('2026-11-02', '2026-11-05'),
    b: days('2026-11-05', '2026-11-06'),
    c: days('2026-11-04'),
  };
  const daysOf = (id: string): Days | undefined => spans[id];

  it('are none without a property', () => {
    const cards = [card('a'), card('b', ['a'])];
    expect(relationsOf(cards, cards, undefined, daysOf).edges).toEqual([]);
    expect(relationsOf(cards, cards, '', daysOf).edges).toEqual([]);
  });

  it('go from the card named to the card that names it, among the cards drawn', () => {
    const all = [card('a'), card('b', ['a', 'gone']), card('c', ['a', 'hidden']), card('hidden')];
    const drawn = all.slice(0, 3);
    const relations = relationsOf(drawn, all, AFTER, daysOf);
    expect(relations.edges).toEqual([
      { from: 'a', to: 'b' },
      { from: 'a', to: 'c' },
    ]);
  });

  it('are out of order where the later card starts before the day the earlier ends', () => {
    const cards = [card('a'), card('b', ['a']), card('c', ['a'])];
    const relations = relationsOf(cards, cards, AFTER, daysOf);
    // b starts on the day a ends: in order. c starts the day before.
    expect([...relations.late]).toEqual([edgeKey({ from: 'a', to: 'c' })]);
    expect(relations.loops.size).toBe(0);
  });

  it('are never out of order with a card that has no date', () => {
    const cards = [card('a'), card('undated', ['a']), card('b', ['undated'])];
    const relations = relationsOf(cards, cards, AFTER, daysOf);
    expect(relations.late.size).toBe(0);
  });

  it('are marked on a loop, one that goes through a card that is not drawn too', () => {
    // a → b → hidden → a, and b → c beside the loop.
    const all = [card('a', ['hidden']), card('b', ['a']), card('hidden', ['b']), card('c', ['b'])];
    const drawn = all.filter((each) => each.id !== 'hidden');
    const relations = relationsOf(drawn, all, AFTER, daysOf);
    expect(relations.edges).toHaveLength(2);
    expect([...relations.loops]).toEqual([edgeKey({ from: 'a', to: 'b' })]);
  });
});

describe('an arrow', () => {
  const bar = (id: string, row: number, x: number, width: number): Bar => ({
    id,
    row,
    x,
    width,
    fromBefore: false,
    toAfter: false,
  });

  it('goes out of the end of the first bar, down, and into the start of the second', () => {
    expect(arrowPoints(bar('a', 0, 2, 4), bar('b', 2, 10, 3))).toEqual([
      [6, 0.5],
      [6 + ARROW_GAP, 0.5],
      [6 + ARROW_GAP, 2.5],
      [10, 2.5],
    ]);
  });

  it('goes up to a card in a row above', () => {
    const points = arrowPoints(bar('a', 3, 2, 4), bar('b', 1, 10, 3));
    expect(points.at(0)).toEqual([6, 3.5]);
    expect(points.at(-1)).toEqual([10, 1.5]);
    expect(points).toHaveLength(4);
  });

  it('turns back between the rows where the second bar starts no later than the first ends', () => {
    // The second starts on the day the first ends: no room to go straight in.
    expect(arrowPoints(bar('a', 0, 2, 4), bar('b', 1, 5, 3))).toEqual([
      [6, 0.5],
      [6 + ARROW_GAP, 0.5],
      [6 + ARROW_GAP, 1],
      [5 - ARROW_GAP, 1],
      [5 - ARROW_GAP, 1.5],
      [5, 1.5],
    ]);
    // Upwards the turn is on the line above the first bar's row.
    expect(arrowPoints(bar('a', 4, 2, 4), bar('b', 1, 1, 3))[2]).toEqual([6 + ARROW_GAP, 4]);
    // A bar at the very start of the axis: the arrow stays on the chart.
    expect(arrowPoints(bar('a', 0, 2, 4), bar('b', 1, 0, 3))[3]).toEqual([0, 1]);
  });

  it('always ends at the start of the second bar, pointing into it', () => {
    const place = fc.record({
      x: fc.double({ min: 0, max: 500, noNaN: true }),
      width: fc.double({ min: MIN_BAR_WIDTH, max: 100, noNaN: true }),
    });
    fc.assert(
      fc.property(place, place, fc.nat(50), fc.nat(50), (a, b, rowA, rowB) => {
        fc.pre(rowA !== rowB);
        const points = arrowPoints(bar('a', rowA, a.x, a.width), bar('b', rowB, b.x, b.width));
        expect(points[0]).toEqual([a.x + a.width, rowA + 0.5]);
        expect(points.at(-1)).toEqual([b.x, rowB + 0.5]);
        // The last piece runs to the right, along the middle of the second row.
        const before = points.at(-2);
        expect(before?.[1]).toBe(rowB + 0.5);
        expect(before?.[0]).toBeLessThanOrEqual(b.x);
        // Every piece is straight across or straight down.
        points.slice(1).forEach((point, at) => {
          const from = points[at];
          expect(point[0] === from?.[0] || point[1] === from?.[1]).toBe(true);
        });
      }),
    );
  });

  it('is drawn once for each edge whose two cards have a bar, with its marks', () => {
    const cards = [card('a'), card('b', ['a']), card('c', ['a', 'b']), card('undated', ['a'])];
    const spans: Record<string, Days> = {
      a: days('2026-11-04', '2026-11-06'),
      b: days('2026-11-05'),
      c: days('2026-11-09'),
    };
    const daysOf = (id: string): Days | undefined => spans[id];
    const made = axis('day');
    const drawn = cards.filter((each) => daysOf(each.id));
    const rows = rowsOf([{ cardIds: drawn.map((each) => each.id) }]);
    const relations = relationsOf(drawn, cards, AFTER, daysOf);
    const arrows = arrowsOf(relations, barsOf(made, rows, daysOf));
    expect(arrows.map(({ from, to, late, loop }) => ({ from, to, late, loop }))).toEqual([
      { from: 'a', to: 'b', late: true, loop: false },
      { from: 'a', to: 'c', late: false, loop: false },
      { from: 'b', to: 'c', late: false, loop: false },
    ]);
    expect(arrows).toHaveLength(relations.edges.length);
  });

  it('is written as the path of its line and of its head', () => {
    const points = arrowPoints(bar('a', 0, 2, 4), bar('b', 2, 10, 3));
    expect(arrowPath(points)).toBe('M6 0.5L6.5 0.5L6.5 2.5L10 2.5');
    expect(arrowHead(points)).toBe('M10 2.5L9.6 2.36L9.6 2.64Z');
    expect(arrowHead([])).toBe('');
  });
});
