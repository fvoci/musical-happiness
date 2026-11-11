/**
 * The cards of a calendar view as the reader's calendar has them: the days
 * of each, the cards of each day the grid shows, and how a card is named.
 * Worked out once per change of the cards, the view or the month, and read
 * by every part that draws (the grid of bars, the small grid, the list of a
 * day), so that none of them goes through all the cards for each day.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import { cardsByDay, compareSpans, spansOf, type Span } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import { displayDay, displayDayRange, displayTime } from '$lib/logic/format.ts';
import type { Regional } from '$lib/logic/regional.ts';
import { parseDateValue, type Card, type PropertyTemplate } from '$lib/logic/wire.ts';

export class Placed {
  /** The cards that have days, in the order of the view. */
  readonly spans: readonly Span[];
  /** The cards with no value for the property they are placed by, in the order of the view. */
  readonly undated: readonly Card[];
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
    const { spans, undated } = spansOf(cards, template, regional.timeZone);
    this.spans = spans;
    this.#byId = new Map(cards.map((card) => [card.id, card]));
    this.undated = undated.flatMap((id) => this.#byId.get(id) ?? []);
    this.#spanOf = new Map(this.spans.map((span) => [span.id, span]));
    this.#byDay = cardsByDay(this.spans, days);
  }

  card(id: string): Card | undefined {
    return this.#byId.get(id);
  }

  span(id: string): Span | undefined {
    return this.#spanOf.get(id);
  }

  /** The cards on a day, in the order of their bars (`compareSpans`, then the view's). */
  on(date: CalendarDate): Card[] {
    const ids = this.#byDay.get(date) ?? cardsByDay(this.spans, [date]).get(date) ?? [];
    const spans = ids.flatMap((id) => this.#spanOf.get(id) ?? []).sort(compareSpans);
    return spans.flatMap((span) => this.#byId.get(span.id) ?? []);
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

  /** The second line of a bar of a week: its time and, for several days, which. */
  detail(card: Card): string | undefined {
    const parts = [this.time(card), this.range(card)].filter((part) => part !== undefined);
    return parts.length > 0 ? parts.join(' · ') : undefined;
  }

  /**
   * What a screen reader hears for a bar: what the bar shows (the time, where
   * the value has one, and the title), then its day or days. A bar stands
   * apart from the cells of its days, so it says them itself.
   */
  name(card: Card): string {
    const span = this.#spanOf.get(card.id);
    const time = this.time(card);
    const title = time
      ? m.calendar_time_title({ time, title: this.title(card) })
      : this.title(card);
    if (!span) return title;
    return m.calendar_card_days({
      title,
      days: this.range(card) ?? displayDay(span.first, this.locale),
    });
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
