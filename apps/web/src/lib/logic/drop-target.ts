/**
 * Where a dragged item would land.
 *
 * Lists of items lie side by side (the columns of a board) and each list
 * stacks its items along one axis. Everything here works on plain numbers:
 * rectangles measured by the caller, in any one coordinate system, and the
 * order of the items as ids. Nothing reads the document.
 *
 * A place is always counted among the items of a list *without* the dragged
 * one, so that "index 2" means the same thing whether the item comes from
 * that list or from another.
 */

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The direction in which a list stacks its items. */
export type Axis = 'x' | 'y';

/** A list on screen: the part of it that is visible, and its items in order, without the dragged one. */
export interface ListBox {
  id: string;
  rect: Rect;
  items: readonly Rect[];
}

/** A place in a list. `index` counts the items of the list without the dragged one. */
export interface DropTarget {
  listId: string;
  index: number;
}

/** The order of the items as data: what a drop changes. */
export interface ListOrder {
  id: string;
  items: readonly string[];
}

export interface DropTargetOptions {
  /** The direction in which each list stacks its items. Default `y`. */
  axis?: Axis;
  /**
   * The visible part of whatever holds the lists. A pointer outside it counts
   * as being on its edge, so that a list scrolled out of view cannot be hit.
   */
  bounds?: Rect;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), Math.max(low, high));
}

/** The point of the rectangle that is nearest to `point`: the point itself when it is inside. */
export function clampToRect(point: Point, rect: Rect): Point {
  return {
    x: clamp(point.x, rect.left, rect.left + rect.width),
    y: clamp(point.y, rect.top, rect.top + rect.height),
  };
}

/** How far `position` is outside the stretch that starts at `start` and is `size` long. */
function distanceOutside(position: number, start: number, size: number): number {
  return Math.max(start - position, position - (start + size), 0);
}

/**
 * The list the point is over, or the one nearest to it when it is over none.
 *
 * Nearest means nearest across the axis first, and only among lists equally
 * near that way, nearest along it. With columns side by side a point below a
 * short column therefore still belongs to that column, however close a taller
 * neighbour is; with lists stacked on top of each other, or laid out as a
 * grid, the distance along the axis decides between those in line with the
 * point. Of two lists at the same distance the earlier one wins. Undefined
 * only when there is no list.
 */
export function nearestList<L extends { rect: Rect }>(
  point: Point,
  lists: readonly L[],
  axis: Axis = 'y',
): L | undefined {
  let best: L | undefined;
  let bestAcross = Infinity;
  let bestAlong = Infinity;
  for (const list of lists) {
    const horizontal = distanceOutside(point.x, list.rect.left, list.rect.width);
    const vertical = distanceOutside(point.y, list.rect.top, list.rect.height);
    const across = axis === 'y' ? horizontal : vertical;
    const along = axis === 'y' ? vertical : horizontal;
    if (across < bestAcross || (across === bestAcross && along < bestAlong)) {
      best = list;
      bestAcross = across;
      bestAlong = along;
    }
  }
  return best;
}

/**
 * The index at which an item dropped at `point` is inserted: the number of
 * items whose middle lies before the point along the axis.
 *
 * `items` are the rectangles of the list's items in order, without the
 * dragged one. With a gap held open at the returned index the answer does not
 * flip back: opening the gap moves the items only further to the side of the
 * point they were already on.
 */
export function insertionIndex(point: Point, items: readonly Rect[], axis: Axis = 'y'): number {
  const position = axis === 'y' ? point.y : point.x;
  const index = items.findIndex((rect) => {
    const middle = axis === 'y' ? rect.top + rect.height / 2 : rect.left + rect.width / 2;
    return position < middle;
  });
  return index === -1 ? items.length : index;
}

/**
 * The place a dragged item would land at when released at `point`.
 *
 * The list is the one under the point or nearest to it; the index comes from
 * the point pulled onto the visible part of that list, so a pointer above a
 * scrolled list lands at its first visible item and not at its hidden start.
 */
export function dropTarget(
  point: Point,
  lists: readonly ListBox[],
  options: DropTargetOptions = {},
): DropTarget | undefined {
  const inside = options.bounds ? clampToRect(point, options.bounds) : point;
  const list = nearestList(inside, lists, options.axis);
  if (!list) return undefined;
  const onList = clampToRect(inside, list.rect);
  return { listId: list.id, index: insertionIndex(onList, list.items, options.axis) };
}

export function sameTarget(first: DropTarget, second: DropTarget): boolean {
  return first.listId === second.listId && first.index === second.index;
}

/** Where an item is now, as a place: its index among the other items of its list. */
export function locateItem(lists: readonly ListOrder[], itemId: string): DropTarget | undefined {
  for (const list of lists) {
    const index = list.items.indexOf(itemId);
    if (index !== -1) return { listId: list.id, index };
  }
  return undefined;
}

/**
 * The order after `itemId` was dropped at `target`. The lists passed in are
 * not changed, and lists the move does not touch are returned as they are.
 * An unknown item or list changes nothing; an index past the end means the end.
 */
export function moveItem(
  lists: readonly ListOrder[],
  itemId: string,
  target: DropTarget,
): ListOrder[] {
  const from = locateItem(lists, itemId);
  if (!from || !lists.some((list) => list.id === target.listId)) return [...lists];
  return lists.map((list) => {
    if (list.id !== from.listId && list.id !== target.listId) return list;
    const items = list.items.filter((id) => id !== itemId);
    if (list.id === target.listId) items.splice(clamp(target.index, 0, items.length), 0, itemId);
    return { ...list, items };
  });
}

/** A step taken with the keyboard while carrying an item. */
export type KeyboardStep = 'previous' | 'next' | 'first' | 'last' | 'previousList' | 'nextList';

/**
 * The place a carried item moves to with one step of the keyboard.
 *
 * `lists` is the order as data, still holding the carried item where it was
 * picked up. Within a list a step stops at the ends. A step to the list on
 * either side keeps the index when that list is long enough and otherwise
 * goes to its end. A step that leads nowhere returns the place unchanged.
 */
export function keyboardTarget(
  current: DropTarget,
  step: KeyboardStep,
  lists: readonly ListOrder[],
  itemId: string,
): DropTarget {
  const at = lists.findIndex((list) => list.id === current.listId);
  const others = (list: ListOrder | undefined): number =>
    list ? list.items.filter((id) => id !== itemId).length : 0;
  const here = others(lists[at]);

  switch (step) {
    case 'previous':
      return { listId: current.listId, index: clamp(current.index - 1, 0, here) };
    case 'next':
      return { listId: current.listId, index: clamp(current.index + 1, 0, here) };
    case 'first':
      return { listId: current.listId, index: 0 };
    case 'last':
      return { listId: current.listId, index: here };
    case 'previousList':
    case 'nextList': {
      const neighbour = lists[step === 'nextList' ? at + 1 : at - 1];
      if (at === -1 || !neighbour) return current;
      return { listId: neighbour.id, index: clamp(current.index, 0, others(neighbour)) };
    }
  }
}

/**
 * How hard a pointer at `position` pushes a scrolling box along one axis.
 *
 * The box starts at `start` and is `size` long. Within `edge` of either end
 * the result grows from 0 at the inner border of that zone to 1 at the end
 * and stays 1 beyond it; it is negative toward the start. In the middle it is
 * 0. A box too short for two zones gives each a third of itself, so that a
 * middle without scrolling always remains.
 */
export function edgeScrollFactor(
  position: number,
  start: number,
  size: number,
  edge: number,
): number {
  const zone = Math.min(edge, size / 3);
  if (zone <= 0) return 0;
  const fromStart = position - start;
  const fromEnd = start + size - position;
  if (fromStart < zone) return -clamp((zone - fromStart) / zone, 0, 1);
  if (fromEnd < zone) return clamp((zone - fromEnd) / zone, 0, 1);
  return 0;
}
