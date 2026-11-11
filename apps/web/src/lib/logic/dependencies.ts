/**
 * The cards of a board as a graph, by one card relation property.
 *
 * A relation value is written on the card that has it and names other cards.
 * Read as "comes after" (a Gantt chart's predecessors), a card B whose value
 * names A gives the edge A → B: A comes first, B after it.
 *
 * - Identifiers of cards that are not there (deleted) are cut: they give no
 *   edge. They stay in the stored value; see `cutIds`.
 * - A card naming itself, or naming a card twice, gives no edge (twice: one).
 * - Loops are not refused by the server and may already be there. The editor
 *   refuses to close a new one where it is told to (`cardsClosingLoop`);
 *   one that is there is only shown (`edgesOnLoops`).
 * - Dates out of order: B starts on a day before the day A ends. Ending and
 *   starting on the same day is in order.
 *
 * Everything here is pure: cards in, answers out.
 */
import type { CalendarDate } from './dates.ts';
import { propertyBehavior } from './properties.ts';
import { cardProperty, parseRelationValue, type Card, type PropertyTemplate } from './wire.ts';

/** `from` is named in the relation value of `to`. */
export interface RelationEdge {
  from: string;
  to: string;
}

/** The identifiers in a card's value that name no card in `cardIds` (deleted ones). */
export function cutIds(card: Card, propertyId: string, cardIds: ReadonlySet<string>): string[] {
  return parseRelationValue(cardProperty(card, propertyId)).filter((id) => !cardIds.has(id));
}

/**
 * The edges of one relation property among `cards`, in the order of the
 * cards and of their values. Identifiers of cards not among `cards`, and a
 * card naming itself, give none.
 */
export function relationEdges(cards: Iterable<Card>, propertyId: string): RelationEdge[] {
  const list = [...cards];
  const ids = new Set(list.map((card) => card.id));
  const edges: RelationEdge[] = [];
  for (const card of list) {
    for (const id of parseRelationValue(cardProperty(card, propertyId))) {
      if (id !== card.id && ids.has(id)) edges.push({ from: id, to: card.id });
    }
  }
  return edges;
}

function successorsOf(edges: readonly RelationEdge[]): Map<string, string[]> {
  const next = new Map<string, string[]>();
  for (const { from, to } of edges) {
    const list = next.get(from);
    if (list) list.push(to);
    else next.set(from, [to]);
  }
  return next;
}

/**
 * The cards that `cardId` may not name without closing a loop: itself and
 * every card that comes after it, however far. Naming one of them adds an
 * edge from it to `cardId`, and there is a way back already.
 */
export function cardsClosingLoop(edges: readonly RelationEdge[], cardId: string): Set<string> {
  const next = successorsOf(edges);
  const reached = new Set([cardId]);
  const waiting = [cardId];
  for (let id = waiting.pop(); id !== undefined; id = waiting.pop()) {
    for (const after of next.get(id) ?? []) {
      if (reached.has(after)) continue;
      reached.add(after);
      waiting.push(after);
    }
  }
  return reached;
}

/** Whether adding the edge `from` → `to` closes a loop. */
export function closesLoop(edges: readonly RelationEdge[], from: string, to: string): boolean {
  return cardsClosingLoop(edges, to).has(from);
}

/**
 * The groups of cards that reach each other (strongly connected
 * components, Tarjan's algorithm without recursion, so that a long chain
 * does not run out of stack). Every card of `edges` is in one group.
 */
function reachingGroups(edges: readonly RelationEdge[]): Map<string, number> {
  const next = successorsOf(edges);
  const nodes = [...new Set(edges.flatMap(({ from, to }) => [from, to]))];
  const index = new Map<string, number>();
  const low = new Map<string, number>();
  const group = new Map<string, number>();
  const stack: string[] = [];
  let counter = 0;
  let groups = 0;
  for (const root of nodes) {
    if (index.has(root)) continue;
    const path: { id: string; at: number }[] = [{ id: root, at: 0 }];
    index.set(root, counter);
    low.set(root, counter);
    counter += 1;
    stack.push(root);
    while (path.length > 0) {
      const top = path[path.length - 1] as { id: string; at: number };
      const after = next.get(top.id)?.[top.at];
      if (after !== undefined) {
        top.at += 1;
        if (!index.has(after)) {
          index.set(after, counter);
          low.set(after, counter);
          counter += 1;
          stack.push(after);
          path.push({ id: after, at: 0 });
        } else if (!group.has(after)) {
          low.set(top.id, Math.min(low.get(top.id) ?? 0, index.get(after) ?? 0));
        }
        continue;
      }
      path.pop();
      const parent = path[path.length - 1];
      if (parent) low.set(parent.id, Math.min(low.get(parent.id) ?? 0, low.get(top.id) ?? 0));
      if (low.get(top.id) !== index.get(top.id)) continue;
      for (let id = stack.pop(); id !== undefined; id = stack.pop()) {
        group.set(id, groups);
        if (id === top.id) break;
      }
      groups += 1;
    }
  }
  return group;
}

/** The edges that lie on a loop: both ends reach each other. */
export function edgesOnLoops(edges: readonly RelationEdge[]): RelationEdge[] {
  const group = reachingGroups(edges);
  return edges.filter(({ from, to }) => group.get(from) === group.get(to));
}

/** The first and last day a card takes by a date property, for a reader in `timeZone`. */
function daysOfCard(card: Card, template: PropertyTemplate, timeZone: string): Days | undefined {
  return propertyBehavior(template.type).days?.(cardProperty(card, template.id), card, timeZone);
}

/** The first and last day of something on the calendar. */
export interface Days {
  first: CalendarDate;
  last: CalendarDate;
}

/** The rule itself: what comes after starts on a day before the day what comes before ends. */
export function daysOutOfOrder(before: Days | undefined, after: Days | undefined): boolean {
  return before !== undefined && after !== undefined && after.first < before.last;
}

/**
 * Whether the dates of `after` are out of order with those of `before`, by a
 * date property: `after` starts on a day before the day `before` ends. A
 * value with a time counts by the day it falls on in `timeZone`. Ending and
 * starting on the same day is in order; a card without a date is never out
 * of order.
 */
export function isOutOfOrder(
  before: Card,
  after: Card,
  template: PropertyTemplate,
  timeZone: string,
): boolean {
  return daysOutOfOrder(
    daysOfCard(before, template, timeZone),
    daysOfCard(after, template, timeZone),
  );
}

/**
 * The edges whose two cards are out of order, by days worked out already
 * (a chart has them for its bars): `daysOf` answers undefined for a card
 * without a date.
 */
export function edgesOutOfOrderByDays(
  edges: readonly RelationEdge[],
  daysOf: (cardId: string) => Days | undefined,
): RelationEdge[] {
  return edges.filter(({ from, to }) => daysOutOfOrder(daysOf(from), daysOf(to)));
}

/** The edges whose two cards are out of order by a date property. */
export function edgesOutOfOrder(
  cards: ReadonlyMap<string, Card>,
  edges: readonly RelationEdge[],
  template: PropertyTemplate,
  timeZone: string,
): RelationEdge[] {
  return edges.filter(({ from, to }) => {
    const before = cards.get(from);
    const after = cards.get(to);
    return before !== undefined && after !== undefined
      ? isOutOfOrder(before, after, template, timeZone)
      : false;
  });
}
