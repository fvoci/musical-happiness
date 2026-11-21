/**
 * Moving a view of a board up or down its list, and where a new view goes.
 *
 * Each view keeps its place in its own `fields.order` (`logic/list-places.ts`),
 * not the board one list of them: a move writes the view moved, so that two
 * people moving two views at once both keep their moves. The board's first
 * move gives every view without a place the place it shows at, in the same
 * step of undo; after it, every move is one write, and a new view goes at the
 * end with its own making. A move shows at once, and its undo puts back what
 * it wrote (a view that had no place has none again).
 */
import { movedTo, placeAtEnd } from '../logic/list-places.ts';

import type { BoardStore } from './board.svelte.ts';
import { placesOf } from './order.ts';

/** Moves a view a place up (`-1`) or down (`1`) its board's list; at an end, nothing. */
export async function moveView(board: BoardStore, viewId: string, step: -1 | 1): Promise<void> {
  const shown = board.orderedViews;
  const from = shown.findIndex((view) => view.id === viewId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= shown.length) return;
  const writes = movedTo(placesOf(shown), viewId, to);
  await board.inOneStep((groupId) =>
    // Not one after another: all of them show at once, as one move.
    Promise.all(
      writes.map(({ id, place }) => board.setViewFields(id, { order: place }, 'moveView', groupId)),
    ),
  );
}

/**
 * The place of a view made now: after the last, once the board's views have
 * places; none on a board whose views nobody moved yet, where a new view goes
 * by its title among them, as the others do.
 */
export function newViewPlace(board: BoardStore): { order?: string } {
  const order = placeAtEnd(placesOf(board.orderedViews));
  return order === undefined ? {} : { order };
}
