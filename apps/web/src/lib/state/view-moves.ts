/**
 * Moving a view of a board up or down its list, adding one at its end, and a
 * copy of one right after it.
 *
 * Each view keeps its place in its own `fields.order` (`logic/list-places.ts`),
 * not the board one list of them: a move writes the view moved, so that two
 * people moving two views at once both keep their moves. The first view a
 * board has moved, added or copied gives every view without a place the
 * place it shows at, and a move into a tie raises the tie's views after it:
 * writes that change nothing shown and are no step of undo
 * (`BoardStore.keepViewPlaces`). The move is the step: it shows at once, and
 * its undo puts the view back where it showed, others' places staying.
 */
import type { BoardView } from '../logic/wire.ts';
import { movedTo, placeAtEnd } from '../logic/list-places.ts';

import type { ViewDraft } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { placesOf, viewCopy } from './order.ts';

/** Moves a view a place up (`-1`) or down (`1`) its board's list; at an end, nothing. */
export async function moveView(board: BoardStore, viewId: string, step: -1 | 1): Promise<void> {
  const shown = board.orderedViews;
  const from = shown.findIndex((view) => view.id === viewId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= shown.length) return;
  const { place, keep, was } = movedTo(placesOf(shown), viewId, to);
  // Both show before their first wait, as one move: what keeps the others where they
  // are, and the move, whose undo puts back where the view showed.
  await Promise.all([
    board.keepViewPlaces(keep),
    board.changeView(
      viewId,
      (view) => ({ ...view, fields: { ...view.fields, order: place } }),
      'moveView',
      undefined,
      { order: was },
    ),
  ]);
}

/** Adds a view at the end of its board's list. */
export async function addViewAtEnd(
  board: BoardStore,
  draft: ViewDraft,
): Promise<BoardView | undefined> {
  const { place, keep } = placeAtEnd(placesOf(board.orderedViews));
  const [made] = await Promise.all([
    board.createView({ ...draft, fields: { ...draft.fields, order: place } }),
    board.keepViewPlaces(keep),
  ]);
  return made;
}

/** A copy of a view under another title, right after it in its board's list. */
export async function duplicateView(
  board: BoardStore,
  viewId: string,
  title: string,
): Promise<BoardView | undefined> {
  const source = board.view(viewId);
  if (!source) return undefined;
  const { fields, keep } = viewCopy(source, board.orderedViews);
  const [copy] = await Promise.all([
    board.createView({ title, fields }),
    board.keepViewPlaces(keep),
  ]);
  return copy;
}
