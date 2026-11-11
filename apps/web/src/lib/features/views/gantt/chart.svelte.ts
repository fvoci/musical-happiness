/**
 * What a Gantt view works out from its cards: the axis, the rows, and the
 * bar of each card. The components lay out; this holds what they lay out
 * from, each step worked out again only when what it reads has changed.
 */
import { displayProperty } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import {
  axisOf,
  barsOf,
  rowsOf,
  scaleOf,
  type Axis,
  type Bar,
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
  /** What a screen reader hears for the bar: the title and the days. */
  name: string;
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
          name: this.placed.name(card),
        },
      ];
    }),
  );
}
