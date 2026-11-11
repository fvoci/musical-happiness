/**
 * What a Gantt view works out from its cards: the axis, the rows, the bar of
 * each card, and the arrows between them. The components lay out; this holds what they lay out
 * from, each step worked out again only when what it reads has changed.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import { displayProperty } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import {
  arrowsOf,
  axisOf,
  barsOf,
  cardsBefore,
  dependencyProperty,
  edgeKey,
  relationsOf,
  rowsOf,
  scaleOf,
  type Arrow,
  type Axis,
  type Bar,
  type ChartRelations,
  type ChartRow,
  type RowList,
  type Scale,
} from '$lib/logic/gantt.ts';
import type { BoardView, Card, GanttZoom, PropertyTemplate } from '$lib/logic/wire.ts';
import type { BoardStore } from '$lib/state/index.ts';

import { Placed } from '../calendar/placed.ts';

export interface ChartSource {
  board: () => BoardStore;
  view: () => BoardView;
  /** The cards the view shows: filtered, searched and sorted. */
  cards: () => readonly Card[];
  /** Today on the reader's calendar. */
  today: () => CalendarDate;
  zoom: () => GanttZoom;
  /** How many days the page has room for. */
  minDays: () => number;
}

/** The row of a card as it is drawn. */
export interface CardLine {
  kind: 'card';
  id: string;
  row: number;
  bar: Bar;
  /** What the row and the bar show. */
  title: string;
  /**
   * What a screen reader hears for the bar: the title, the days, the cards
   * it comes after, and what is wrong with that (out of order, on a loop).
   */
  name: string;
  /** The card starts before the day a card it comes after ends. */
  late: boolean;
  /** Following what the card comes after leads back to the card. */
  loop: boolean;
}

export type ChartLine = CardLine;

export class GanttChart {
  readonly #source: ChartSource;

  constructor(source: ChartSource) {
    this.#source = source;
  }

  get #board(): BoardStore {
    return this.#source.board();
  }

  readonly #templates = $derived.by(() => this.#board.board?.cardProperties ?? []);
  readonly #regional = $derived.by(() => this.#board.workspace.regional);

  /** The property the bars are placed by; none: by when the cards were made. */
  readonly template = $derived.by<PropertyTemplate | undefined>(() =>
    displayProperty(this.#source.view().fields.dateDisplayPropertyId, this.#templates),
  );

  /** The days of each card for the reader, and the cards that have none. */
  readonly placed = $derived.by(
    () =>
      new Placed(
        this.#source.cards(),
        this.template,
        this.#regional,
        this.#board.workspace.locale,
        [],
      ),
  );

  readonly axis = $derived.by<Axis>(() =>
    axisOf({
      zoom: this.#source.zoom(),
      weekStart: this.#regional.weekStart,
      today: this.#source.today(),
      spans: this.placed.spans,
      minDays: this.#source.minDays(),
    }),
  );

  readonly scale = $derived.by<Scale>(() => scaleOf(this.axis, this.#regional.weekStart));

  /** The cards that have days, as the lists between headings. */
  readonly #lists = $derived.by<RowList[]>(() => [
    { cardIds: this.placed.spans.map((span) => span.id) },
  ]);

  readonly rows = $derived.by<ChartRow[]>(() => rowsOf(this.#lists));

  readonly bars = $derived.by(() =>
    barsOf(this.axis, this.rows, (cardId) => this.placed.span(cardId)),
  );

  /** The card relation read as what comes before what; none: no arrows. */
  readonly dependency = $derived.by<PropertyTemplate | undefined>(() =>
    dependencyProperty(this.#source.view().fields.dependencyPropertyId, this.#templates),
  );

  /** The edges between the cards that have a bar. Loops are looked for on the whole board. */
  readonly relations = $derived.by<ChartRelations>(() =>
    relationsOf(this.#source.cards(), this.#board.realCards, this.dependency?.id, (cardId) =>
      this.placed.span(cardId),
    ),
  );

  readonly arrows = $derived.by<Arrow[]>(() =>
    arrowsOf(this.relations, this.bars, this.axis.width),
  );

  /** For each card, the cards it comes after, in the order of its value. */
  readonly #before = $derived.by(() => cardsBefore(this.relations.edges));

  /** The titles of cards as a list in the reader's language: "A, B and C". */
  #listed(cardIds: readonly string[]): string {
    const titles = cardIds.flatMap((id) => {
      const card = this.placed.card(id);
      return card ? [this.placed.title(card)] : [];
    });
    return new Intl.ListFormat(this.#board.workspace.locale, { type: 'conjunction' }).format(
      titles,
    );
  }

  /** The cards that `cardId` comes after and starts too early for. */
  #lateBefore(cardId: string): string[] {
    const before = this.#before.get(cardId) ?? [];
    return before.filter((from) => this.relations.late.has(edgeKey({ from, to: cardId })));
  }

  /**
   * What a screen reader hears for a bar: its title and days, the cards it
   * comes after, and in words what the dashes and the marks say to the eye.
   */
  #name(card: Card): string {
    const before = this.#before.get(card.id);
    const late = this.#lateBefore(card.id);
    const name = this.placed.name(card);
    return [
      before ? m.gantt_bar_after({ name, cards: this.#listed(before) }) : name,
      late.length > 0 ? m.gantt_bar_late({ cards: this.#listed(late) }) : undefined,
      this.relations.loopCards.has(card.id) ? m.gantt_bar_loop() : undefined,
    ]
      .filter((part) => part !== undefined)
      .join(' ');
  }

  /** The rows as drawn. */
  readonly lines = $derived.by<ChartLine[]>(() =>
    this.rows.flatMap((row): ChartLine[] => {
      const card = row.kind === 'card' ? this.placed.card(row.id) : undefined;
      const bar = this.bars.get(row.id);
      if (!card || !bar) return [];
      return [
        {
          kind: 'card',
          id: row.id,
          row: row.row,
          bar,
          title: this.placed.title(card),
          name: this.#name(card),
          late: this.#lateBefore(row.id).length > 0,
          loop: this.relations.loopCards.has(row.id),
        },
      ];
    }),
  );

  /** The cards with a bar that are out of order, and those on a loop, from the top. */
  readonly lateIds = $derived.by(() =>
    this.lines.filter((line) => line.late).map((line) => line.id),
  );
  readonly loopIds = $derived.by(() =>
    this.lines.filter((line) => line.loop).map((line) => line.id),
  );
}
