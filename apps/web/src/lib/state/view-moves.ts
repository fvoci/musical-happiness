/**
 * Creating, moving, and duplicating board views using single-transaction
 * server fractional-indexing endpoints (`writeView`, `moveView`, `duplicateView`).
 */
import { movedTo } from '../logic/list-places.ts';
import type { BoardView } from '../logic/wire.ts';

import { isBackendError, type BoardViewWriteResult, type ViewDraft } from './backend.ts';
import { deleteBlockForUndo, performBoardAction } from './board-sync.ts';
import type { BoardStore } from './board.svelte.ts';
import { placesOf } from './order.ts';
import { put, take } from './write.ts';

interface MoveTurnQueue {
  tail: Promise<unknown>;
  inFlight: number;
}

const moveQueues = new WeakMap<BoardStore, MoveTurnQueue>();

function queueOf(board: BoardStore): MoveTurnQueue {
  let queue = moveQueues.get(board);
  if (!queue) {
    queue = { tail: Promise.resolve(), inFlight: 0 };
    moveQueues.set(board, queue);
  }
  return queue;
}

function takeViewWrite(board: BoardStore, written: BoardViewWriteResult): BoardView {
  let next = board.views;
  for (const companion of written.companionViews) next = take(next, companion);
  next = take(next, written.view);
  board.views = next;
  return written.view;
}

function applyOptimisticMove(board: BoardStore, viewId: string, step: -1 | 1): void {
  const shown = board.orderedViews;
  const from = shown.findIndex((view) => view.id === viewId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= shown.length) return;
  const { place, keep } = movedTo(placesOf(shown), viewId, to);
  let next = board.views;
  for (const item of keep) {
    const cur = next.find((v) => v.id === item.id);
    if (cur && cur.fields.order !== item.place) {
      next = put(next, { ...cur, fields: { ...cur.fields, order: item.place } });
    }
  }
  const target = next.find((v) => v.id === viewId);
  if (target) next = put(next, { ...target, fields: { ...target.fields, order: place } });
  board.views = next;
}

async function runQueuedMove(board: BoardStore, viewId: string, after: string): Promise<void> {
  const queue = queueOf(board);
  queue.inFlight += 1;
  const turn = queue.tail.then(async () => {
    try {
      const written = await board.workspace.backend.moveView(board.boardId, viewId, after);
      if (queue.inFlight === 1) takeViewWrite(board, written);
    } catch (error) {
      if (queue.inFlight === 1) {
        await board.rereadViews().catch(() => undefined);
      }
      throw error;
    } finally {
      queue.inFlight -= 1;
    }
  });
  queue.tail = turn.catch(() => undefined);
  await turn;
}

/**
 * Moves a view one slot up (`-1`) or down (`1`) in `board.orderedViews` via
 * `POST /boards/{boardId}/views/{viewId}/move`, showing the new order
 * synchronously, serializing overlapping moves per board, and re-reading on refusal.
 */
export async function moveView(board: BoardStore, viewId: string, step: -1 | 1): Promise<boolean> {
  const shown = board.orderedViews;
  const from = shown.findIndex((view) => view.id === viewId);
  const to = from + step;
  if (from < 0 || to < 0 || to >= shown.length) return false;

  const without = shown.filter((view) => view.id !== viewId);
  const forwardAfter = to === 0 ? '' : (without[to - 1]?.id ?? '');
  const backwardAfter = from === 0 ? '' : (without[from - 1]?.id ?? '');

  applyOptimisticMove(board, viewId, step);
  let firstCall = true;
  let ok = false;
  await performBoardAction(
    board,
    async () => {
      if (!firstCall) applyOptimisticMove(board, viewId, step);
      firstCall = false;
      await runQueuedMove(board, viewId, forwardAfter);
      ok = true;
    },
    async () => {
      applyOptimisticMove(board, viewId, -step as -1 | 1);
      await runQueuedMove(board, viewId, backwardAfter);
    },
    'moveView',
  );
  return ok;
}

/** Adds a view at the end of `board.orderedViews` via `POST /boards/{boardId}/views`. */
export function addViewAtEnd(board: BoardStore, draft: ViewDraft): Promise<BoardView | undefined> {
  return performBoardAction(
    board,
    async () => {
      const written = await board.workspace.backend.writeView(board.boardId, {
        ...draft,
        after: null,
      });
      return takeViewWrite(board, written);
    },
    async (created) => {
      await deleteBlockForUndo(board, created.id);
      board.views = board.views.filter((existing) => existing.id !== created.id);
    },
    'createView',
  );
}

/** Duplicates `viewId` under `title` right after `viewId` via `POST .../views/{viewId}/duplicate`. */
export function duplicateView(
  board: BoardStore,
  viewId: string,
  title: string,
): Promise<BoardView | undefined> {
  if (!board.view(viewId)) return Promise.resolve(undefined);
  return performBoardAction(
    board,
    async () => {
      const written = await board.workspace.backend.duplicateView(
        board.boardId,
        viewId,
        title,
        viewId,
      );
      return takeViewWrite(board, written);
    },
    async (copied) => {
      try {
        await deleteBlockForUndo(board, copied.id);
      } catch (error) {
        if (!isBackendError(error) || error.status !== 404) throw error;
      }
      board.views = board.views.filter((existing) => existing.id !== copied.id);
    },
    'createView',
  );
}

/** Optimistically applies a single view replacement for callers holding a `BoardView`. */
export function showBoardView(board: BoardStore, view: BoardView): void {
  board.views = put(board.views, view);
}
