/**
 * The quick filters of a board: chips a reader turns on to narrow the cards
 * of whatever view they look at, for themselves alone. They are not the
 * view's filter, which every member of the board sees: the view is not
 * changed, and nothing of them goes to the server.
 *
 * - `mine`: a person property of the card names the reader.
 * - `incomplete`: the card is not done by the board's rule (`doneWhen`).
 * - `overdue`: the card is due before the reader's today and not done.
 * - `dueThisWeek`: the card is due in the reader's week (`regional.weekStart`).
 *
 * A card is due on the last day of its due date (the board's
 * `dueDatePropertyId`): a range is due when it ends. Chips that are on all
 * have to hold.
 */
import { weekOf } from './calendar.ts';
import { dateOfInstant, daysOf, type CalendarDate } from './dates.ts';
import { doneRuleOf, isCardDone } from './filter-done.ts';
import type { Weekday } from './regional.ts';
import { cardProperty, parseDateValue, type Card, type Container } from './wire.ts';

export const QUICK_FILTERS = ['mine', 'incomplete', 'overdue', 'dueThisWeek'] as const;

export type QuickFilterId = (typeof QUICK_FILTERS)[number];

export function isQuickFilterId(value: unknown): value is QuickFilterId {
  return QUICK_FILTERS.some((id) => id === value);
}

/** What the chips read of a board. */
export type QuickFilterBoard = Pick<Container, 'cardProperties' | 'doneWhen' | 'dueDatePropertyId'>;

/** What the chips read of the reader and the moment. */
export interface QuickFilterContext {
  /** The reader; none on a public link, where nobody is "me". */
  meId?: string | undefined;
  timeZone: string;
  weekStart: Weekday;
  /** The current moment in milliseconds; the clock's when not given. */
  now?: number | undefined;
}

/** The person properties of a board: those a card names its people in. */
function personPropertyIds(board: QuickFilterBoard): string[] {
  return board.cardProperties
    .filter((template) => template.type === 'person' || template.type === 'multiPerson')
    .map((template) => template.id);
}

/** The board's due date property, when it has one that holds dates. */
function duePropertyId(board: QuickFilterBoard): string | undefined {
  const id = board.dueDatePropertyId;
  return board.cardProperties.some((template) => template.id === id && template.type === 'date')
    ? (id ?? undefined)
    : undefined;
}

/**
 * The chips a board offers its reader, in the order they are shown: "mine"
 * to a reader who is signed in, on a board with a person property; the
 * others where the board has what they read (a rule of done, a due date).
 */
export function offeredQuickFilters(
  board: QuickFilterBoard | undefined,
  meId: string | undefined,
): QuickFilterId[] {
  if (!board) return [];
  const hasDone = doneRuleOf(board.doneWhen, board.cardProperties) !== undefined;
  const hasDue = duePropertyId(board) !== undefined;
  const offered: Record<QuickFilterId, boolean> = {
    mine: meId !== undefined && personPropertyIds(board).length > 0,
    incomplete: hasDone,
    overdue: hasDue,
    dueThisWeek: hasDue,
  };
  return QUICK_FILTERS.filter((id) => offered[id]);
}

/** The day a card is due for the reader: the last day of its due date. */
function dueDayOf(board: QuickFilterBoard, card: Card, timeZone: string): CalendarDate | undefined {
  const id = duePropertyId(board);
  if (id === undefined) return undefined;
  return daysOf(parseDateValue(cardProperty(card, id)), timeZone)?.last;
}

/** Whether a property value names `userId`: a person, or one of several. */
function names(value: unknown, userId: string): boolean {
  return Array.isArray(value) ? value.includes(userId) : value === userId;
}

/** Whether `card` meets the chip `id`. A chip the board cannot evaluate lets every card through. */
export function isQuickFilterMet(
  id: QuickFilterId,
  board: QuickFilterBoard,
  card: Card,
  context: QuickFilterContext,
): boolean {
  const rule = doneRuleOf(board.doneWhen, board.cardProperties);
  const done = rule !== undefined && isCardDone(rule, card);
  const today = (): CalendarDate => dateOfInstant(context.now ?? Date.now(), context.timeZone);
  switch (id) {
    case 'mine': {
      const { meId } = context;
      if (meId === undefined) return true;
      return personPropertyIds(board).some((propertyId) =>
        names(cardProperty(card, propertyId), meId),
      );
    }
    case 'incomplete':
      return !done;
    case 'overdue': {
      const due = dueDayOf(board, card, context.timeZone);
      return due !== undefined && due < today() && !done;
    }
    case 'dueThisWeek': {
      const due = dueDayOf(board, card, context.timeZone);
      if (due === undefined) return false;
      const week = weekOf(today(), context.weekStart);
      return (week[0] ?? due) <= due && due <= (week[6] ?? due);
    }
  }
}

/**
 * The cards that meet every chip of `chosen` the board offers, in their
 * order. A chip it does not offer (one turned on for it before, on a board
 * that has no due date now) is left out: it is not shown, so it cannot be
 * turned off. A card `kept` is let through whatever the chips are: one the
 * reader made on this visit does not vanish as it is made.
 */
export function applyQuickFilters(
  cards: readonly Card[],
  chosen: readonly QuickFilterId[],
  board: QuickFilterBoard | undefined,
  context: QuickFilterContext,
  kept: (card: Card) => boolean = () => false,
): Card[] {
  if (!board || chosen.length === 0) return [...cards];
  const offered = offeredQuickFilters(board, context.meId);
  const applied = chosen.filter((id) => offered.includes(id));
  if (applied.length === 0) return [...cards];
  return cards.filter(
    (card) => kept(card) || applied.every((id) => isQuickFilterMet(id, board, card, context)),
  );
}
