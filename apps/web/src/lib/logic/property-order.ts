/**
 * The order of a board's card properties: what a move up or down makes of
 * it, and a board with its properties in an order (`reorderCardProperties`).
 */
import type { Board } from './wire.ts';

/** The identifiers with one moved a place up (`-1`) or down (`1`); as they are at an end. */
export function movedProperty(ids: readonly string[], propertyId: string, step: -1 | 1): string[] {
  const from = ids.indexOf(propertyId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= ids.length) return [...ids];
  const next = [...ids];
  next.splice(from, 1);
  next.splice(to, 0, propertyId);
  return next;
}

/** The board with its properties in `order`; one the order does not name keeps its place after those it names. */
export function withPropertyOrder(board: Board, order: readonly string[]): Board {
  const rank = new Map(order.map((id, at) => [id, at]));
  const ranked = (id: string): number => rank.get(id) ?? order.length;
  const cardProperties = board.cardProperties
    .map((template, at) => ({ template, at }))
    .sort(
      (first, second) =>
        ranked(first.template.id) - ranked(second.template.id) || first.at - second.at,
    )
    .map(({ template }) => template);
  return { ...board, cardProperties };
}

/** Whether two orders are the same. */
export const sameOrder = (first: readonly string[], second: readonly string[]): boolean =>
  first.length === second.length && first.every((id, at) => second[at] === id);
