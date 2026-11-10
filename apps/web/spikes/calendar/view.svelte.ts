/**
 * What the calendar is showing: which month, which day is picked, month or
 * week. One object for the whole page, outside of every part that draws it.
 *
 * That is the point of it. The grid of a wide pane, the small grid of a
 * narrow one and the list of a day are different parts, and a window that
 * changes size swaps one for another; the month in view and the day picked
 * must not go with the part that happened to hold them.
 *
 * The reader's settings come in as plain values (`resolveRegional`): the time
 * zone decides which day is today and which day a moment falls on, the first
 * day of the week decides the columns.
 */
import { addDays, dateOfInstant, daysOf, type CalendarDate } from '$lib/logic/dates.ts';
import { resolveRegional, type Weekday } from '$lib/logic/regional.ts';

import { CURRENT } from './current.ts';
import { NOW, SHOWN, entryOf, spansFor, type Entry } from './data.ts';
import {
  firstOf,
  idsOn,
  inMonth,
  monthOf,
  shiftMonth,
  weekOf,
  weeksOfMonth,
  type Month,
  type Span,
} from './grid.ts';
import { room } from './room.svelte.ts';
import { SHOW } from './show.ts';

const { screen, variant } = CURRENT;
const second = variant === 'b';

/** The second reader of the `regional` screen: a week from Monday, in Los Angeles. */
const elsewhere = screen === 'regional' && second;

const regional = resolveRegional(
  {
    timeZone: CURRENT.timeZone ?? (elsewhere ? 'America/Los_Angeles' : 'Asia/Seoul'),
    weekStart: CURRENT.weekStart ?? (elsewhere ? 'monday' : 'auto'),
    hourCycle: 'auto',
  },
  document.documentElement.lang,
  Intl.DateTimeFormat().resolvedOptions().timeZone,
);

export type Mode = 'month' | 'week';

class CalendarView {
  readonly timeZone: string = regional.timeZone;
  readonly weekStart: Weekday = regional.weekStart;
  /** Today, for this reader. */
  readonly today: CalendarDate = dateOfInstant(NOW, regional.timeZone);
  /** The days of every dated card, for this reader. */
  readonly spans: readonly Span[] = spansFor(regional.timeZone);

  month = $state<Month>(monthOf(this.today));
  selected = $state<CalendarDate>(this.today);
  mode = $state<Mode>(screen === 'week' ? 'week' : 'month');

  readonly weeks = $derived(weeksOfMonth(this.month, this.weekStart));
  readonly week = $derived(weekOf(this.selected, this.weekStart));
  /**
   * The box is too narrow for bars and shows a list of days under a strip of
   * one week: as a proposal for the month, and always for the view of a week.
   */
  readonly agenda = $derived(!room.bars && (SHOW.narrow === 'agenda' || this.mode === 'week'));
  /** The arrows step by a week. */
  readonly byWeek = $derived(this.mode === 'week' || this.agenda);

  /** The cards on a day, in the order of the data. */
  on(date: CalendarDate): Entry[] {
    return idsOn(date, this.spans).flatMap((id) => entryOf(id) ?? []);
  }

  /** The first and last day of a card for this reader. */
  daysOf(entry: Entry): { first: CalendarDate; last: CalendarDate } | undefined {
    return entry.value && daysOf(entry.value, this.timeZone);
  }

  select(date: CalendarDate): void {
    this.selected = date;
    if (!inMonth(date, this.month)) this.month = monthOf(date);
  }

  /** One month on or back; one week where the view is of a week. */
  step(by: number): void {
    if (this.byWeek) {
      this.select(addDays(this.selected, 7 * by));
      return;
    }
    this.month = shiftMonth(this.month, by);
    this.selected = inMonth(this.today, this.month) ? this.today : firstOf(this.month);
  }

  goToday(): void {
    this.month = monthOf(this.today);
    this.selected = this.today;
  }
}

export const view = new CalendarView();

// The day each screen is about is picked when the page loads.
if (screen === 'more') view.select(SHOWN.crowded);
if (screen === 'create') view.select(SHOWN.empty);
if (screen === 'range') view.select(SHOWN.crowded);
const moved = entryOf(SHOWN.moved);
if (screen === 'move' && moved) view.select(view.daysOf(moved)?.first ?? view.today);
