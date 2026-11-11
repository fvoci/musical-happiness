/**
 * The cards of a calendar view as the reader's calendar has them: the days
 * of each, the cards of each day the grid shows, and how a card is named.
 * Worked out once per change of the cards, the view or the month, and read
 * by every part that draws (the grid of bars, the small grid, the list of a
 * day), so that none of them goes through all the cards for each day.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import { cardsByDay, spansOf, type Span } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import { displayDay, displayDayRange, displayTime } from '$lib/logic/format.ts';
import type { Regional } from '$lib/logic/regional.ts';
import { parseDateValue, type Card, type PropertyTemplate } from '$lib/logic/wire.ts';

export class Placed {
  /** The cards that have days, in the order of the view. */
  readonly spans: readonly Span[];
  /** The property the cards are placed by; none: by when they were made. */
  readonly template: PropertyTemplate | undefined;
  readonly regional: Regional;
  readonly locale: string;
  readonly #byId: Map<string, Card>;
  readonly #spanOf: Map<string, Span>;
  readonly #byDay: Map<CalendarDate, string[]>;

  constructor(
    cards: readonly Card[],
    template: PropertyTemplate | undefined,
    regional: Regional,
    locale: string,
    /** The days the grid shows. */
    days: readonly CalendarDate[],
  ) {
    this.template = template;
    this.regional = regional;
    this.locale = locale;
    this.spans = spansOf(cards, template, regional.timeZone).spans;
    this.#byId = new Map(cards.map((card) => [card.id, card]));
    this.#spanOf = new Map(this.spans.map((span) => [span.id, span]));
    this.#byDay = cardsByDay(this.spans, days);
  }

  card(id: string): Card | undefined {
    return this.#byId.get(id);
  }

  span(id: string): Span | undefined {
    return this.#spanOf.get(id);
  }

  /** The cards on a day, in the order of the view. */
  on(date: CalendarDate): Card[] {
    const ids = this.#byDay.get(date) ?? cardsByDay(this.spans, [date]).get(date) ?? [];
    return ids.flatMap((id) => this.#byId.get(id) ?? []);
  }

  count(date: CalendarDate): number {
    return (this.#byDay.get(date) ?? cardsByDay(this.spans, [date]).get(date) ?? []).length;
  }

  /** The title of a card, or the words for one without a title. */
  title(card: Card): string {
    return card.title || m['CalendarCard.untitled']();
  }

  /**
   * The time a card starts at, on the reader's clock: only for a date
   * property whose value carries one. A time of creation is not written on
   * every bar.
   */
  time(card: Card): string | undefined {
    if (this.template?.type !== 'date') return undefined;
    const instant = parseDateValue(card.fields.properties[this.template.id]).from?.instant;
    if (instant === undefined) return undefined;
    const { timeZone, hourCycle } = this.regional;
    return displayTime(instant, { locale: this.locale, timeZone, hourCycle });
  }

  /** "November 9 – 13" for a card of several days; nothing for one of one day. */
  range(card: Card): string | undefined {
    const span = this.#spanOf.get(card.id);
    if (!span || span.first === span.last) return undefined;
    return displayDayRange(span.first, span.last, this.locale);
  }

  /**
   * What a screen reader hears for a bar: the title, the day or the days,
   * and the time where the value has one. A bar stands apart from the cells
   * of its days, so it says them itself.
   */
  name(card: Card): string {
    const span = this.#spanOf.get(card.id);
    const title = this.title(card);
    if (!span) return title;
    const day = this.range(card) ?? displayDay(span.first, this.locale);
    const time = this.time(card);
    return m.calendar_card_days({ title, days: time ? m.calendar_day_time({ day, time }) : day });
  }

  /** What a screen reader hears for a day: its name and how many cards are on it. */
  dayName(date: CalendarDate, label: string): string {
    const count = this.count(date);
    if (count === 0) return label;
    return count === 1
      ? m.calendar_cards_on_one({ day: label })
      : m.calendar_cards_on({ day: label, count });
  }
}
