/**
 * Moving a view of a board up or down its list, adding one at its end, and a
 * copy of one right after it.
 *
 * Each view keeps its place in its own `fields.order` (`logic/list-places.ts`),
 * not the board one list of them: a move writes the view moved, so that two
 * people moving two views at once both keep their moves. Every place write
 * carries the `updateAt` of the view as it was read when the place was
 * computed (`readAt`), so a stale write is refused (409) instead of
 * overwriting another editor's move. On 409, a move re-reads the board's views
 * and tries the gesture once from the stored order; on a second 409 it stops
 * and shows the stored order. The first view a board has moved, added or
 * copied gives every view without a place the place it shows at, and a move
 * into a tie raises the tie's views after it: writes that change nothing shown
 * and are no step of undo (`keepViewPlaces`). The move is the step: it shows
 * at once, and its undo puts the view back where it showed, others' places
 * staying.
 */
import { isPlace, movedTo, placeAtEnd, type PlaceWrite } from '../logic/list-places.ts';
import type { BoardView } from '../logic/wire.ts';

import { isConflict, type ViewDraft } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { orderViews, placesOf, viewCopy } from './order.ts';
import { isEmptyBlockPatch, viewPatches } from './patches.ts';

interface KeptOutcome {
  stale: PlaceWrite[];
  refused: unknown;
}

/** Says once the refusal of the places kept for a gesture (`keepViewPlaces`). */
function sayKept(board: BoardStore, refused: unknown): void {
  if (refused !== undefined) board.workspace.report('save', refused);
}

/**
 * Writes places that keep views where they show (`Placing.keep`): no step of
 * undo. Each write carries the `updateAt` of the view when the places were
 * computed (`before`), separating 409 conflicts from other refusals.
 */
async function keepViewPlaces(
  board: BoardStore,
  keep: readonly PlaceWrite[],
  before: ReadonlyMap<string, BoardView>,
): Promise<KeptOutcome> {
  const sent: { write: PlaceWrite; running: Promise<void> }[] = [];
  for (const write of keep) {
    const current = board.view(write.id);
    const readAt = before.get(write.id)?.updateAt;
    if (!current || readAt === undefined) continue;
    const placed = { ...current, fields: { ...current.fields, order: write.place } };
    const [forward, backward] = viewPatches(placed, current);
    if (isEmptyBlockPatch(forward)) continue;
    sent.push({
      write,
      running: board.writeView(write.id, { ...forward, readAt }, backward),
    });
  }
  const settled = await Promise.allSettled(sent.map((entry) => entry.running));
  const stale: PlaceWrite[] = [];
  let refused: unknown;
  settled.forEach((result, index) => {
    if (result.status !== 'rejected') return;
    const entry = sent[index];
    if (entry && isConflict(result.reason)) stale.push(entry.write);
    else if (refused === undefined) refused = result.reason;
  });
  return { stale, refused };
}

/**
 * Reconciles `keep` writes that hit 409 after the main gesture succeeded:
 * re-reads the board's views and retries once only those whose `order` did not
 * change meanwhile (for instance, somebody only renamed the view); a view
 * whose `order` already changed keeps the other writer's place.
 */
async function retryStaleKeep(
  board: BoardStore,
  stale: readonly PlaceWrite[],
  before: ReadonlyMap<string, BoardView>,
  skipId?: string,
): Promise<unknown> {
  try {
    await board.rereadViews();
  } catch (error) {
    return error;
  }
  const fresh = new Map(board.orderedViews.map((view) => [view.id, view]));
  const retry = stale.filter(({ id }) => {
    if (id === skipId) return false;
    const now = fresh.get(id);
    return now !== undefined && now.fields.order === before.get(id)?.fields.order;
  });
  if (retry.length === 0) return undefined;
  const again = await keepViewPlaces(board, retry, fresh);
  if (again.stale.length > 0) await board.rereadViews().catch(() => undefined);
  return again.refused;
}

/**
 * Writes a view's moved place as one step of undo, carrying `readAt` from when
 * the place was computed. Undo and redo carry the view's `updateAt` as it is
 * when they run; on 409 they re-read the board's views and refuse so another
 * editor's concurrent move is not overwritten.
 */
async function placeView(
  board: BoardStore,
  viewId: string,
  place: string,
  readAt: number,
): Promise<{ ok: true } | { ok: false; error: unknown }> {
  const current = board.view(viewId);
  if (!current) return { ok: false, error: undefined };
  const placed = { ...current, fields: { ...current.fields, order: place } };
  const [forward, backward] = viewPatches(placed, current);
  if (isEmptyBlockPatch(forward)) return { ok: true };
  let first = true;
  try {
    await board.workspace.undo.perform(
      async () => {
        const at = first ? readAt : board.view(viewId)?.updateAt;
        const initial = first;
        first = false;
        const patch = at === undefined ? forward : { ...forward, readAt: at };
        try {
          await board.writeView(viewId, patch, backward);
        } catch (error) {
          if (!initial && isConflict(error)) await board.rereadViews().catch(() => undefined);
          throw error;
        }
      },
      async () => {
        const at = board.view(viewId)?.updateAt;
        const patch = at === undefined ? backward : { ...backward, readAt: at };
        try {
          await board.writeView(viewId, patch, forward);
        } catch (error) {
          if (isConflict(error)) await board.rereadViews().catch(() => undefined);
          throw error;
        }
      },
      'moveView',
      undefined,
      false,
      board.keepViewInverse(viewId, backward),
    );
    return { ok: true };
  } catch (error) {
    return { ok: false, error };
  }
}

async function moveOnce(
  board: BoardStore,
  viewId: string,
  step: -1 | 1,
  mayRetry: boolean,
  fallbackPlace?: string,
): Promise<boolean> {
  const real = board.view(viewId);
  if (!real) return false;
  const unplaced = !isPlace(real.fields.order);
  // If an unplaced view hit 409 because of a non-order edit while our `keep` placed the
  // other views, it would drop to the end as unplaced; keep its pre-move slot on retry.
  const shown =
    fallbackPlace !== undefined && unplaced
      ? orderViews(
          board.views.map((each) =>
            each.id === viewId
              ? { ...each, fields: { ...each.fields, order: fallbackPlace } }
              : each,
          ),
          board.workspace.locale,
        )
      : board.orderedViews;
  const from = shown.findIndex((view) => view.id === viewId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= shown.length) return false;
  const before = new Map(board.views.map((view) => [view.id, view]));
  const placed = placesOf(shown);
  const { place, keep, was } = movedTo(placed, viewId, to);
  // Where the view shows now is stored first where it is not (it had no place, or the
  // others' places pass it): what an undo, and a refusal, put back. A view's writes go
  // one after another (`BlockTurns.patchView`), forwarding `readAt`, and all of them show at once.
  const own = !unplaced && placed[from]?.place === was ? [] : [{ id: viewId, place: was }];
  const [kept, moved] = await Promise.all([
    keepViewPlaces(board, [...keep, ...own], before),
    placeView(board, viewId, place, real.updateAt),
  ]);
  if (!moved.ok) {
    if (isConflict(moved.error)) {
      await board.rereadViews().catch(() => undefined);
      if (mayRetry) return moveOnce(board, viewId, step, false, unplaced ? was : undefined);
    }
    if (moved.error !== undefined) board.workspace.report('save', moved.error);
    return false;
  }
  const retried =
    kept.stale.length > 0 ? await retryStaleKeep(board, kept.stale, before, viewId) : undefined;
  // One gesture, said once: the move refused is said above; the places kept for it, where it held.
  sayKept(board, kept.refused ?? retried);
  return true;
}

/**
 * Moves a view a place up (`-1`) or down (`1`) its board's list; at an end,
 * nothing. Whether the move held: refused, the view shows where it is stored.
 */
export function moveView(board: BoardStore, viewId: string, step: -1 | 1): Promise<boolean> {
  return moveOnce(board, viewId, step, true);
}

/** Adds a view at the end of its board's list. */
export async function addViewAtEnd(
  board: BoardStore,
  draft: ViewDraft,
): Promise<BoardView | undefined> {
  const shown = board.orderedViews;
  const before = new Map(shown.map((view) => [view.id, view]));
  const { place, keep } = placeAtEnd(placesOf(shown));
  const [made, kept] = await Promise.all([
    board.createView({ ...draft, fields: { ...draft.fields, order: place } }),
    keepViewPlaces(board, keep, before),
  ]);
  if (made) {
    const retried =
      kept.stale.length > 0 ? await retryStaleKeep(board, kept.stale, before) : undefined;
    sayKept(board, kept.refused ?? retried);
  }
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
  const shown = board.orderedViews;
  const before = new Map(shown.map((view) => [view.id, view]));
  const { fields, keep } = viewCopy(source, shown);
  const [copy, kept] = await Promise.all([
    board.createView({ title, fields }),
    keepViewPlaces(board, keep, before),
  ]);
  if (copy) {
    const retried =
      kept.stale.length > 0 ? await retryStaleKeep(board, kept.stale, before) : undefined;
    sayKept(board, kept.refused ?? retried);
  }
  return copy;
}
