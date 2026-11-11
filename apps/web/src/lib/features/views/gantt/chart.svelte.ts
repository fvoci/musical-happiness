/**
 * What a Gantt view works out from its cards: the axis, the rows, the bar of
 * each card, and the arrows between them. The components lay out; this holds what they lay out
 * from, each step worked out again only when what it reads has changed.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import { displayProperty } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import type { BoardGroup } from '$lib/logic/group.ts';
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

/** The row that starts a group. */
export interface GroupLine {
  kind: 'group';
  id: string;
  row: number;
  /** What the group is called: the option, the person, or "No Status". */
  name: string;
  /** The colour name of the option; undefined for a group that is not an option. */
  color: string | undefined;
  /** How many cards of the group have a bar. */
  count: number;
}

export type ChartLine = CardLine | GroupLine;

const PERSON_KINDS: readonly string[] = ['person', 'createdBy', 'updatedBy'];

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

  /** The property the view groups by, when the board still has it; as the table reads it. */
  readonly groupBy = $derived.by<PropertyTemplate | undefined>(() => {
    const id = this.#source.view().fields.groupById;
    return id ? this.#templates.find((template) => template.id === id) : undefined;
  });

  /** The groups of the cards that have days, each in the order of the view; none: not grouped. */
  readonly #groups = $derived.by<BoardGroup[] | undefined>(() => {
    if (!this.groupBy) return undefined;
    const dated = this.placed.spans.flatMap((span) => this.placed.card(span.id) ?? []);
    return this.#board.groupsOf(this.#source.view(), dated, (property) =>
      m['BoardComponent.no-property']({ property }),
    ).visible;
  });

  /** The cards that have days, as the lists between headings. */
  readonly #lists = $derived.by<RowList[]>(() =>
    this.#groups
      ? this.#groups.map((group) => ({
          groupId: group.option.id,
          cardIds: group.cards.map((card) => card.id),
        }))
      : [{ cardIds: this.placed.spans.map((span) => span.id) }],
  );

  /** The heading of a group as drawn: named and coloured as the table does. */
  #groupLine(row: Extract<ChartRow, { kind: 'group' }>): GroupLine[] {
    const option = this.#groups?.find((group) => group.option.id === row.id)?.option;
    if (!option || !this.groupBy) return [];
    const person = option.id !== '' && PERSON_KINDS.includes(this.groupBy.type);
    return [
      {
        ...row,
        name: person
          ? (this.#board.workspace.userName(option.id) ?? m['centerPanel.unknown-user']())
          : option.value,
        color: person || option.id === '' ? undefined : option.color,
      },
    ];
  }

  readonly rows = $derived.by<ChartRow[]>(() => rowsOf(this.#lists));

  readonly bars = $derived.by(() =>
    barsOf(this.axis, this.rows, (cardId) => this.placed.span(cardId)),
  );

  /** The cards of the rows: a card of a group the view hides has days and no row. */
  readonly #rowCards = $derived.by<Card[]>(() =>
    this.rows.flatMap((row) => (row.kind === 'card' ? (this.placed.card(row.id) ?? []) : [])),
  );

  /** The card relation read as what comes before what; none: no arrows. */
  readonly dependency = $derived.by<PropertyTemplate | undefined>(() =>
    dependencyProperty(this.#source.view().fields.dependencyPropertyId, this.#templates),
  );

  /** The edges between the cards that have a bar. Loops are looked for on the whole board. */
  readonly relations = $derived.by<ChartRelations>(() =>
    relationsOf(this.#rowCards, this.#board.realCards, this.dependency?.id, (cardId) =>
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
      if (row.kind === 'group') return this.#groupLine(row);
      const card = this.placed.card(row.id);
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

  /** The rows of cards, from the top: what the arrow keys walk. */
  readonly cardLines = $derived.by(() => this.lines.filter((line) => line.kind === 'card'));

  /** The cards with a bar that are out of order, and those on a loop, from the top. */
  readonly lateIds = $derived.by(() =>
    this.cardLines.filter((line) => line.late).map((line) => line.id),
  );
  readonly loopIds = $derived.by(() =>
    this.cardLines.filter((line) => line.loop).map((line) => line.id),
  );
}
